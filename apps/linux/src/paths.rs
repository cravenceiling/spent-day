use std::path::{Path, PathBuf};

/// `$SPENTDAY_DB`, else `$XDG_DATA_HOME/spentday/spentday.db`. The default folder is Spent Day's
/// own, so it is made private (`0700`), since the database holds the sync code. A folder named by
/// `$SPENTDAY_DB` is the user's and is left alone. The core creates any missing folder `0700` and
/// keeps the database files `0600` either way.
pub fn db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SPENTDAY_DB") {
        return PathBuf::from(path);
    }
    let dir = gtk::glib::user_data_dir().join("spentday");
    if let Err(e) = private_dir(&dir) {
        eprintln!("spentday: cannot make {} private: {e}", dir.display());
    }
    dir.join("spentday.db")
}

/// The core takes the database path as a string. A lossy conversion could name another file,
/// so a path that is not UTF-8 is refused.
pub fn utf8(path: &Path) -> Result<String, spentday_core::SpentDayError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        spentday_core::SpentDayError::InvalidArgument(
            "The database path is not valid UTF-8. Set SPENTDAY_DB or XDG_DATA_HOME to a path that is.".into(),
        )
    })
}

/// Creates `dir` and any missing parents as `0700`, and sets an existing `dir` to `0700`,
/// whatever the umask.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::fs::{DirBuilder, Permissions, set_permissions};
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    set_permissions(dir, Permissions::from_mode(0o700))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_path_that_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/not-utf8-\xff.db"));
        let e = utf8(bad).unwrap_err();
        assert!(e.to_string().contains("not valid UTF-8"), "{e}");
        assert_eq!(utf8(Path::new("/tmp/caf\u{e9}.db")).unwrap(), "/tmp/caf\u{e9}.db");
    }

    #[test]
    fn data_dir_is_private_when_new_and_when_it_was_not() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("share/spentday");
        private_dir(&dir).unwrap();
        assert_eq!((mode(&dir), mode(dir.parent().unwrap())), (0o700, 0o700));

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
    }
}
