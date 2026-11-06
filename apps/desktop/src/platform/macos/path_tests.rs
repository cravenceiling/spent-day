use super::test_support::Scratch;
use super::*;
#[test]
fn folder_is_private_on_creation_and_upgrade() {
    let dir = Scratch::new();
    let folder = dir.0.join("SpentDay");
    make_private_folder(&folder).unwrap();
    assert_eq!(
        fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    make_private_folder(&folder).unwrap();
    assert_eq!(
        fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(folder_name(Some("day.spent.SpentDay")), "SpentDay");
    assert_eq!(folder_name(Some("day.spent.SpentDay.dev")), "SpentDay Dev");
}
#[test]
fn folder_failures_stop_startup() {
    let dir = Scratch::new();
    let locked = dir.0.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    let result = make_private_folder(&locked.join("SpentDay"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.unwrap_err().contains(&format!(
        "couldn't make its folder {}/SpentDay private",
        locked.display()
    )));
    let file = dir.0.join("file");
    fs::write(&file, []).unwrap();
    assert!(make_private_folder(&file).is_err());
}
#[test]
fn override_is_verbatim_and_leaves_parent_permissions_alone() {
    let dir = Scratch::new();
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    let path = dir.0.join("custom.db").into_os_string();
    assert_eq!(
        resolve_db_path(Some(path.clone()), || panic!(
            "override uses no default folder"
        ))
        .unwrap(),
        path.to_str().unwrap()
    );
    assert_eq!(
        fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
        0o755
    );
    use std::os::unix::ffi::OsStringExt;
    assert!(
        resolve_db_path(
            Some(std::ffi::OsString::from_vec(vec![0xff])),
            || unreachable!()
        )
        .is_err()
    );
}
