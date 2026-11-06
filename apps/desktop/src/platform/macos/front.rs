use objc2_app_kit::NSWorkspace;
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{NSArray, NSDictionary};

use super::native::{dictionary, number};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontApp {
    pub id: String,
    pub name: String,
    pub pid: i32,
}

pub fn current() -> Option<FrontApp> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let name = app.localizedName().map(|s| s.to_string());
    let id = app
        .bundleIdentifier()
        .map(|s| s.to_string())
        .or_else(|| {
            app.executableURL()?
                .lastPathComponent()
                .map(|s| s.to_string())
        })
        .or_else(|| name.clone())?;
    Some(FrontApp {
        name: name.unwrap_or_else(|| id.clone()),
        id,
        pid: app.processIdentifier(),
    })
}

pub fn window_number(pid: i32) -> Option<i64> {
    let windows = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    )?;
    // CGWindow's immutable CF collections are toll-free bridged Foundation collections.
    let windows =
        unsafe { &*((&*windows as *const objc2_core_foundation::CFArray).cast::<NSArray>()) };
    for value in windows {
        let Some(window) = value.downcast_ref::<NSDictionary>() else {
            continue;
        };
        if number(window, "kCGWindowOwnerPID") == Some(i64::from(pid))
            && number(window, "kCGWindowLayer") == Some(0)
        {
            return number(window, "kCGWindowNumber");
        }
    }
    None
}

pub fn session_state() -> Option<(bool, bool)> {
    let session = objc2_core_graphics::CGSessionCopyCurrentDictionary()?;
    let session = dictionary(&session);
    Some((
        number(session, "CGSSessionScreenIsLocked").unwrap_or(0) != 0,
        number(session, "kCGSSessionOnConsoleKey").unwrap_or(1) == 0,
    ))
}
