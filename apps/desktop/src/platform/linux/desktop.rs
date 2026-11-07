//! What `.desktop` files say about an app id: its display name, so `org.gnome.Nautilus` shows
//! as "Files", its icon, and the desktop id MPRIS players report.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    pub name: Option<String>,
    /// A theme icon name or an absolute path, as the entry's `Icon` key gives it.
    pub icon: Option<String>,
    /// The entry's file name without `.desktop`, such as `org.gnome.Nautilus`.
    pub desktop_id: Option<String>,
}

pub struct Entries {
    dirs: Vec<PathBuf>,
    langs: Vec<String>,
    /// Lowercase file stems and `StartupWMClass` values to their files. Built on first use.
    index: Option<HashMap<String, PathBuf>>,
    cache: HashMap<String, Info>,
}

impl Entries {
    pub fn new() -> Entries {
        Entries::with(
            application_dirs(),
            langs(&env::var("LANG").unwrap_or_default()),
        )
    }

    fn with(dirs: Vec<PathBuf>, langs: Vec<String>) -> Entries {
        Entries {
            dirs,
            langs,
            index: None,
            cache: HashMap::new(),
        }
    }

    /// The entry's `Name`, or the id itself when no entry matches.
    pub fn name(&mut self, app_id: &str) -> String {
        self.info(app_id)
            .name
            .clone()
            .unwrap_or_else(|| app_id.to_owned())
    }

    /// First an exact `<id>.desktop`, then a case-insensitive file name or `StartupWMClass`
    /// match, which covers X11 classes like `Chromium` or `jetbrains-idea`, then the last part
    /// of a reverse-DNS file name, so the class `mousepad` finds `org.xfce.mousepad.desktop`.
    pub fn info(&mut self, app_id: &str) -> &Info {
        if !self.cache.contains_key(app_id) {
            let info = self
                .path(app_id)
                .and_then(|p| read(&p, &self.langs))
                .unwrap_or_default();
            self.cache.insert(app_id.to_owned(), info);
        }
        &self.cache[app_id]
    }

    pub fn path(&mut self, app_id: &str) -> Option<PathBuf> {
        let exact = format!("{app_id}.desktop");
        self.dirs
            .iter()
            .map(|d| d.join(&exact))
            .find(|p| p.is_file())
            .or_else(|| {
                let dirs = &self.dirs;
                self.index
                    .get_or_insert_with(|| index(dirs))
                    .get(&app_id.to_lowercase())
                    .cloned()
            })
    }
}

fn application_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs =
        env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut dirs = vec![data_home.clone()];
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    // A `systemd --user` service may not inherit the session's XDG_DATA_DIRS.
    dirs.push(data_home.join("flatpak/exports/share"));
    dirs.push("/var/lib/flatpak/exports/share".into());
    dirs.push("/var/lib/snapd/desktop".into());
    dirs.into_iter().map(|d| d.join("applications")).collect()
}

/// `de_AT.UTF-8` → `["de_AT", "de"]`, the order `Name[...]` keys are tried in.
fn langs(lang: &str) -> Vec<String> {
    let lang = lang.split(['.', '@']).next().unwrap_or_default();
    if lang.is_empty() || lang == "C" || lang == "POSIX" {
        return Vec::new();
    }
    let mut out = vec![lang.to_owned()];
    if let Some((short, _)) = lang.split_once('_') {
        out.push(short.to_owned());
    }
    out
}

/// The `[Desktop Entry]` group's keys, first occurrence wins.
fn keys(text: &str) -> HashMap<&str, &str> {
    let mut keys = HashMap::new();
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            keys.entry(key.trim()).or_insert(value.trim());
        }
    }
    keys
}

fn read(path: &Path, langs: &[String]) -> Option<Info> {
    let text = fs::read_to_string(path).ok()?;
    let keys = keys(&text);
    let name = langs
        .iter()
        .find_map(|lang| keys.get(format!("Name[{lang}]").as_str()))
        .or_else(|| keys.get("Name"))
        .map(|s| s.to_string());
    Some(Info {
        name,
        icon: keys
            .get("Icon")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        desktop_id: path.file_stem().map(|s| s.to_string_lossy().into_owned()),
    })
}

/// Earlier directories win, as they do for the exact lookup. A whole file name or class wins
/// over the last part of another file's name.
fn index(dirs: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    let files: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
        .collect();
    let stem = |path: &Path| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    };
    for path in &files {
        let class = fs::read_to_string(path)
            .ok()
            .and_then(|t| keys(&t).get("StartupWMClass").map(|c| c.to_lowercase()));
        for key in std::iter::once(stem(path)).chain(class) {
            index.entry(key).or_insert_with(|| path.clone());
        }
    }
    for path in &files {
        if let Some((_, last)) = stem(path).rsplit_once('.') {
            index.entry(last.to_owned()).or_insert_with(|| path.clone());
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(lang: &str) -> Entries {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("linux/tests/fixtures/applications");
        Entries::with(vec![PathBuf::from("/nonexistent"), dir], langs(lang))
    }

    #[test]
    fn exact_desktop_id() {
        assert_eq!(
            entries("C").info("org.gnome.Nautilus"),
            &Info {
                name: Some("Files".into()),
                icon: Some("org.gnome.Nautilus".into()),
                desktop_id: Some("org.gnome.Nautilus".into()),
            }
        );
    }

    #[test]
    fn localized_name() {
        assert_eq!(entries("de_DE.UTF-8").name("org.gnome.Nautilus"), "Dateien");
        assert_eq!(
            entries("pt_BR.UTF-8").name("org.gnome.Nautilus"),
            "Arquivos"
        );
    }

    #[test]
    fn x11_class_with_different_case() {
        assert_eq!(entries("C").name("Chromium"), "Chromium Web Browser");
        let path = entries("C").path("Chromium").unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .eq_ignore_ascii_case("chromium.desktop")
        );
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains("Name=Chromium Web Browser")
        );
    }

    #[test]
    fn startup_wm_class() {
        assert_eq!(
            entries("C").info("jetbrains-idea"),
            &Info {
                name: Some("IntelliJ IDEA".into()),
                icon: Some("/opt/idea/bin/idea.svg".into()),
                desktop_id: Some("idea".into()),
            }
        );
    }

    #[test]
    fn last_part_of_a_reverse_dns_name() {
        assert_eq!(entries("C").name("mousepad"), "Mousepad");
        assert_eq!(
            entries("C").info("mousepad").icon.as_deref(),
            Some("org.xfce.mousepad")
        );
    }

    #[test]
    fn unknown_id_is_its_own_name() {
        assert_eq!(entries("C").name("SpentDayA"), "SpentDayA");
        assert_eq!(entries("C").info("SpentDayA"), &Info::default());
        assert_eq!(entries("C").path("SpentDayA"), None);
    }
}
