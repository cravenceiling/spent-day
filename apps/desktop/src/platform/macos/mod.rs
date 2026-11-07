mod browser;
mod collector;
mod front;
mod native;
mod presence;
mod quit;
mod recorder;
pub mod status_item;

use super::{Instance, OnActivate, OnFatal, Pref};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{
    NSBundle, NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask, NSString,
    NSUserDefaults,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use spentday_core::{Platform, SpentDay};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

pub use collector::Tracking;
pub const PLATFORM: Platform = Platform::Macos;
pub const DEVICE_NOUN: &str = "Mac";

pub fn db_path() -> Result<String, String> {
    resolve_db_path(std::env::var_os("SPENTDAY_DB"), || {
        autoreleasepool(|_| {
            let manager = NSFileManager::defaultManager();
            let urls = manager.URLsForDirectory_inDomains(
                NSSearchPathDirectory::ApplicationSupportDirectory,
                NSSearchPathDomainMask::UserDomainMask,
            );
            let path = urls
                .firstObject()
                .and_then(|url| url.path())
                .ok_or("Couldn't find Application Support.")?;
            let id = NSBundle::mainBundle()
                .bundleIdentifier()
                .map(|id| id.to_string());
            Ok(PathBuf::from(path.to_string()).join(folder_name(id.as_deref())))
        })
    })
}
fn folder_name(bundle: Option<&str>) -> &'static str {
    if bundle == Some("day.spent.SpentDay.dev") {
        "SpentDay Dev"
    } else {
        "SpentDay"
    }
}
fn resolve_db_path(
    override_path: Option<std::ffi::OsString>,
    folder: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<String, String> {
    let path = if let Some(path) = override_path {
        PathBuf::from(path)
    } else {
        let folder = folder()?;
        make_private_folder(&folder)?;
        folder.join("spentday.db")
    };
    path.into_os_string()
        .into_string()
        .map_err(|_| "The database path isn't valid UTF-8.".into())
}
fn make_private_folder(folder: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .and_then(|_| fs::set_permissions(folder, fs::Permissions::from_mode(0o700)))
        .map_err(|error| {
            format!(
                "Spent Day couldn't make its folder {} private. {error}",
                folder.display()
            )
        })
}
// NSHost preserves the existing localized device name.
#[allow(deprecated)]
pub fn device_name() -> String {
    use objc2_foundation::NSHost;
    autoreleasepool(|_| {
        NSHost::currentHost()
            .localizedName()
            .map_or_else(|| "Mac".into(), |name| name.to_string())
    })
}
pub fn start_tracking(core: Arc<SpentDay>, on_fatal: OnFatal) -> Tracking {
    collector::start(core, on_fatal)
}
pub fn icon_path(_app_key: &str, app_ids: &[String]) -> Option<PathBuf> {
    autoreleasepool(|_| {
        app_ids.iter().find_map(|id| {
            NSWorkspace::sharedWorkspace()
                .URLForApplicationWithBundleIdentifier(&NSString::from_str(id))?
                .path()
                .map(|p| PathBuf::from(p.to_string()))
        })
    })
}
pub struct InstanceGuard;
pub fn claim_instance(_demo: bool, _show: bool, _on_activate: OnActivate) -> Instance {
    Instance::Primary(InstanceGuard)
}
impl Pref {
    fn key(self) -> &'static str {
        match self {
            Self::OnboardingDone => "onboardingDone",
            Self::RecordSafariSites => "recordSafariSites",
        }
    }
}
pub fn read_pref(p: Pref) -> Option<bool> {
    Some(autoreleasepool(|_| {
        NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(p.key()))
    }))
}
pub fn write_pref(p: Pref, on: bool) {
    autoreleasepool(|_| {
        NSUserDefaults::standardUserDefaults().setBool_forKey(on, &NSString::from_str(p.key()))
    });
}
pub fn autostart() -> Option<bool> {
    Some(unsafe {
        matches!(
            SMAppService::mainAppService().status(),
            SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval
        )
    })
}
pub fn set_autostart(on: bool) -> Result<Option<String>, String> {
    unsafe {
        let service = SMAppService::mainAppService();
        let registered = matches!(
            service.status(),
            SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval
        );
        if on != registered {
            (if on {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            })
            .map_err(|e| {
                format!(
                    "macOS didn't accept the change: {}",
                    e.localizedDescription()
                )
            })?;
        }
        Ok(
            (service.status() == SMAppServiceStatus::RequiresApproval).then(|| {
                "Allow Spent Day in System Settings > General > Login Items to finish.".into()
            }),
        )
    }
}
pub fn menu_bar(on_click: Box<dyn Fn([f64; 4])>) {
    status_item::install(on_click);
}
pub fn install_quit_handler(quit: Box<dyn Fn()>) {
    quit::install(quit);
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
#[cfg(test)]
#[path = "path_tests.rs"]
mod path_tests;
#[cfg(test)]
#[path = "recorder_tests.rs"]
mod recorder_tests;

#[cfg(test)]
mod test_support;
