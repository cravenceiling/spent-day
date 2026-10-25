//! Canonical app table. Maps platform app ids (macOS bundle ids, Linux WM_CLASS
//! and desktop ids, Android packages) to one key per app so the dashboard can
//! group the same app across devices.

use std::collections::HashMap;
use std::sync::LazyLock;

pub struct App {
    pub key: &'static str,
    pub name: &'static str,
    pub browser: bool,
    ids: &'static [&'static str],
}

const APPS: &[App] = &[
    App {
        key: "safari",
        name: "Safari",
        browser: true,
        ids: &["com.apple.Safari", "com.apple.SafariTechnologyPreview"],
    },
    App {
        key: "chrome",
        name: "Chrome",
        browser: true,
        ids: &[
            "com.google.Chrome",
            "com.google.Chrome.beta",
            "com.google.Chrome.dev",
            "com.google.Chrome.canary",
            "google-chrome",
            "google-chrome-stable",
            "google-chrome-beta",
            "google-chrome-unstable",
            "com.android.chrome",
            "com.chrome.beta",
            "com.chrome.dev",
            "com.chrome.canary",
        ],
    },
    App {
        key: "chromium",
        name: "Chromium",
        browser: true,
        ids: &[
            "org.chromium.Chromium",
            "chromium",
            "chromium-browser",
            "org.chromium.chrome",
        ],
    },
    App {
        key: "firefox",
        name: "Firefox",
        browser: true,
        ids: &[
            "org.mozilla.firefox",
            "org.mozilla.firefoxdeveloperedition",
            "org.mozilla.nightly",
            "firefox",
            "firefox-esr",
            "firefox-developer-edition",
            "firefox-nightly",
            "org.mozilla.firefox_beta",
            "org.mozilla.fenix",
        ],
    },
    App {
        key: "brave",
        name: "Brave",
        browser: true,
        ids: &[
            "com.brave.Browser",
            "brave-browser",
            "brave",
            "com.brave.browser_beta",
            "com.brave.browser_nightly",
        ],
    },
    App {
        key: "helium",
        name: "Helium",
        browser: true,
        ids: &["net.imput.helium", "helium"],
    },
    App {
        key: "edge",
        name: "Microsoft Edge",
        browser: true,
        ids: &[
            "com.microsoft.edgemac",
            "com.microsoft.edgemac.Beta",
            "com.microsoft.edgemac.Dev",
            "microsoft-edge",
            "microsoft-edge-stable",
            "microsoft-edge-beta",
            "microsoft-edge-dev",
            "com.microsoft.Edge",
            "com.microsoft.emmx",
        ],
    },
    App {
        key: "arc",
        name: "Arc",
        browser: true,
        ids: &["company.thebrowser.Browser", "company.thebrowser.arc"],
    },
    App {
        key: "opera",
        name: "Opera",
        browser: true,
        ids: &[
            "com.operasoftware.Opera",
            "com.operasoftware.OperaGX",
            "opera",
            "com.opera.Opera",
            "com.opera.browser",
            "com.opera.mini.native",
            "com.opera.gx",
        ],
    },
    App {
        key: "vivaldi",
        name: "Vivaldi",
        browser: true,
        ids: &[
            "com.vivaldi.Vivaldi",
            "vivaldi",
            "vivaldi-stable",
            "com.vivaldi.browser",
        ],
    },
    App {
        key: "zen",
        name: "Zen",
        browser: true,
        ids: &[
            "app.zen-browser.zen",
            "app.zen_browser.zen",
            "io.github.zen_browser.zen",
            "zen",
            "zen-browser",
            "zen-alpha",
        ],
    },
    App {
        key: "samsung-internet",
        name: "Samsung Internet",
        browser: true,
        ids: &[
            "com.sec.android.app.sbrowser",
            "com.sec.android.app.sbrowser.beta",
            "samsung",
            "samsung-internet",
        ],
    },
    App {
        key: "duckduckgo",
        name: "DuckDuckGo",
        browser: true,
        ids: &[
            "com.duckduckgo.mobile.android",
            "com.duckduckgo.macos.browser",
        ],
    },
    App {
        key: "slack",
        name: "Slack",
        browser: false,
        ids: &[
            "com.tinyspeck.slackmacgap",
            "slack",
            "com.slack.Slack",
            "com.Slack",
        ],
    },
    App {
        key: "discord",
        name: "Discord",
        browser: false,
        ids: &["com.hnc.Discord", "discord", "com.discordapp.Discord"],
    },
    App {
        key: "spotify",
        name: "Spotify",
        browser: false,
        ids: &["com.spotify.client", "spotify", "com.spotify.music"],
    },
    App {
        key: "vscode",
        name: "Visual Studio Code",
        browser: false,
        ids: &[
            "com.microsoft.VSCode",
            "code",
            "code-oss",
            "com.visualstudio.code",
        ],
    },
    App {
        key: "telegram",
        name: "Telegram",
        browser: false,
        ids: &[
            "ru.keepcoder.Telegram",
            "org.telegram.desktop",
            "telegram-desktop",
            "org.telegram.messenger",
        ],
    },
    App {
        key: "signal",
        name: "Signal",
        browser: false,
        ids: &[
            "org.whispersystems.signal-desktop",
            "signal",
            "org.signal.Signal",
            "org.thoughtcrime.securesms",
        ],
    },
    App {
        key: "whatsapp",
        name: "WhatsApp",
        browser: false,
        ids: &["net.whatsapp.WhatsApp", "com.whatsapp"],
    },
    App {
        key: "zoom",
        name: "Zoom",
        browser: false,
        ids: &[
            "us.zoom.xos",
            "zoom",
            "us.zoom.Zoom",
            "us.zoom.videomeetings",
        ],
    },
];

/// Lowercased id, without a `.desktop` suffix. WM_CLASS casing varies by toolkit
/// (`Google-chrome` vs `google-chrome`), so matching is case-insensitive.
fn normalize_id(app_id: &str) -> String {
    let id = app_id.trim().to_ascii_lowercase();
    id.strip_suffix(".desktop").map(str::to_owned).unwrap_or(id)
}

static BY_ID: LazyLock<HashMap<String, &'static App>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for app in APPS {
        map.insert(app.key.to_owned(), app);
        for id in app.ids {
            map.insert(normalize_id(id), app);
        }
    }
    map
});

pub fn lookup(app_id: &str) -> Option<&'static App> {
    BY_ID.get(&normalize_id(app_id)).copied()
}

pub fn is_browser(app_id: &str) -> bool {
    lookup(app_id).is_some_and(|app| app.browser)
}

/// Canonical key for a known app, the id itself otherwise.
pub fn app_key(app_id: &str) -> String {
    lookup(app_id).map_or_else(|| app_id.to_owned(), |app| app.key.to_owned())
}

/// Canonical key of the browser a web hint names, or `None` when the name is not
/// in the table.
pub fn browser_key(browser: &str) -> Option<&'static str> {
    lookup(browser).filter(|app| app.browser).map(|app| app.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_platform_ids_under_one_key() {
        assert_eq!(app_key("com.google.Chrome"), "chrome");
        assert_eq!(app_key("Google-chrome"), "chrome");
        assert_eq!(app_key("google-chrome.desktop"), "chrome");
        assert_eq!(app_key("com.android.chrome"), "chrome");
        assert_eq!(app_key("org.mozilla.firefox"), "firefox");
        assert_eq!(app_key("com.sec.android.app.sbrowser"), "samsung-internet");
        assert_eq!(app_key("app.zen-browser.zen"), "zen");
        assert_eq!(app_key("com.example.Unknown"), "com.example.Unknown");
    }

    #[test]
    fn knows_which_apps_are_browsers() {
        assert!(is_browser("company.thebrowser.Browser"));
        assert!(is_browser("com.microsoft.emmx"));
        assert!(is_browser("net.imput.helium"));
        assert!(!is_browser("com.tinyspeck.slackmacgap"));
        assert!(!is_browser("com.example.Unknown"));
        assert_eq!(browser_key("firefox"), Some("firefox"));
        assert_eq!(browser_key("slack"), None);
        assert_eq!(browser_key("netscape"), None);
    }

    #[test]
    fn every_id_maps_to_exactly_one_app() {
        let mut seen = HashMap::new();
        for app in APPS {
            for id in app.ids.iter().copied().chain([app.key]) {
                if let Some(other) = seen.insert(normalize_id(id), app.key) {
                    assert_eq!(other, app.key, "{id} is listed under two apps");
                }
            }
        }
    }
}
