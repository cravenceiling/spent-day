//! The platform seam: the same plain functions on each OS, picked at compile time. No trait,
//! since a build never has more than one implementation.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("Spent Day desktop builds for macOS and Linux only");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pref {
    OnboardingDone,
    RecordSafariSites,
}

pub enum Instance {
    Primary(InstanceGuard),
    /// Another process owns the app; it got this launch's `show` and activation token. macOS
    /// leaves single instance to LaunchServices.
    #[cfg(target_os = "linux")]
    Forwarded,
}

/// Called with `(show, activation token)` when a later launch forwards to this one.
pub type OnActivate = Box<dyn Fn(bool, String) + Send + Sync>;
/// Called at most once, on any thread, when collection can't start.
pub type OnFatal = Box<dyn FnOnce(String) + Send>;
