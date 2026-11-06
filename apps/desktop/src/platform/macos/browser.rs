use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use objc2::{
    AnyThread,
    rc::{Retained, autoreleasepool},
};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleScript, NSAppleScriptErrorNumber, NSNumber, NSString,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserTab {
    Page(String),
    NoPage,
}

pub const BUNDLE_IDS: [&str; 11] = [
    "com.apple.Safari",
    "com.apple.SafariTechnologyPreview",
    "com.brave.Browser",
    "com.google.Chrome",
    "com.google.Chrome.beta",
    "com.google.Chrome.canary",
    "com.microsoft.edgemac",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    "company.thebrowser.Browser",
    "net.imput.helium",
];
pub fn is_safari(bundle: &str) -> bool {
    matches!(
        bundle,
        "com.apple.Safari" | "com.apple.SafariTechnologyPreview"
    )
}
pub fn source(bundle: &str) -> Option<String> {
    if !BUNDLE_IDS.contains(&bundle) {
        return None;
    }
    let dialect = if is_safari(bundle) {
        "return {URL of current tab of w}"
    } else if bundle == "company.thebrowser.Browser" {
        "if incognito of w then return {}\nreturn {URL of active tab of w}"
    } else {
        "if mode of w is \"incognito\" then return {}\nreturn {URL of active tab of w}"
    };
    Some(format!(
        "with timeout of 2 seconds\n    tell application id \"{bundle}\"\n        set w to window id (id of front window)\n{dialect}\n    end tell\nend timeout"
    ))
}
pub fn decode(result: &NSAppleEventDescriptor) -> BrowserTab {
    match result
        .descriptorAtIndex(1)
        .and_then(|d| d.stringValue())
        .map(|s| s.to_string())
    {
        Some(url) if url.starts_with("https://") || url.starts_with("http://") => {
            BrowserTab::Page(url)
        }
        _ => BrowserTab::NoPage,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Asked {
    pid: i32,
    activation: u64,
    window: Option<i64>,
}
struct Entry {
    tab: Option<BrowserTab>,
    at: Instant,
    asked: Asked,
}
#[derive(Default)]
struct Cache {
    latest: HashMap<String, Entry>,
    pending: HashSet<String>,
}
struct Request {
    bundle: String,
    asked: Asked,
}
type Window = Arc<dyn Fn(i32) -> Option<i64> + Send + Sync>;
type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

pub struct BrowserPages {
    cache: Arc<Mutex<Cache>>,
    requests: mpsc::Sender<Request>,
    window: Window,
    clock: Clock,
}
impl BrowserPages {
    pub fn new(answered: impl Fn() + Send + 'static) -> Self {
        Self::spawn(
            Arc::new(super::front::window_number),
            Arc::new(Instant::now),
            || {
                let mut scripts = Scripts::default();
                move |id: &str| scripts.run(id)
            },
            answered,
        )
    }
    pub(super) fn spawn<Q: FnMut(&str) -> Result<BrowserTab, i32> + 'static>(
        window: Window,
        clock: Clock,
        make_query: impl FnOnce() -> Q + Send + 'static,
        answered: impl Fn() + Send + 'static,
    ) -> Self {
        let cache = Arc::new(Mutex::new(Cache::default()));
        let (requests, receive) = mpsc::channel::<Request>();
        let (worker_cache, worker_window, worker_clock) =
            (cache.clone(), window.clone(), clock.clone());
        std::thread::Builder::new()
            .name("spentday-browser".into())
            .spawn(move || {
                let mut query = autoreleasepool(|_| make_query());
                let mut backoff = HashMap::new();
                for request in receive {
                    autoreleasepool(|_| {
                        let now = worker_clock();
                        let mut tab = if backoff
                            .get(&request.bundle)
                            .is_some_and(|until| *until > now)
                        {
                            None
                        } else {
                            match query(&request.bundle) {
                                Ok(tab) => Some(tab),
                                Err(-1728) => Some(BrowserTab::NoPage),
                                Err(code) => {
                                    backoff.insert(
                                        request.bundle.clone(),
                                        worker_clock()
                                            + Duration::from_secs(if code == -1743 {
                                                600
                                            } else {
                                                30
                                            }),
                                    );
                                    None
                                }
                            }
                        };
                        if worker_window(request.asked.pid) != request.asked.window {
                            tab = None;
                        }
                        {
                            let mut cache = worker_cache.lock().unwrap();
                            cache.pending.remove(&request.bundle);
                            cache.latest.insert(
                                request.bundle,
                                Entry {
                                    tab,
                                    at: worker_clock(),
                                    asked: request.asked,
                                },
                            );
                        }
                        answered();
                    });
                }
            })
            .expect("start browser worker");
        Self {
            cache,
            requests,
            window,
            clock,
        }
    }
    pub fn tab(&self, bundle: &str, pid: i32, activation: u64) -> Option<BrowserTab> {
        source(bundle)?;
        let asked = Asked {
            pid,
            activation,
            window: (self.window)(pid),
        };
        let mut cache = self.cache.lock().unwrap();
        let now = (self.clock)();
        let last = cache.latest.get(bundle).filter(|entry| {
            entry.asked == asked && now.duration_since(entry.at) < Duration::from_secs(10)
        });
        let tab = last.and_then(|entry| entry.tab.clone());
        let refresh =
            last.is_none_or(|entry| now.duration_since(entry.at) >= Duration::from_secs(1));
        if refresh && !cache.pending.contains(bundle) {
            cache.pending.insert(bundle.to_owned());
            let _ = self.requests.send(Request {
                bundle: bundle.to_owned(),
                asked,
            });
        }
        tab
    }
}
#[derive(Default)]
struct Scripts {
    compiled: HashMap<String, Retained<NSAppleScript>>,
}
impl Scripts {
    fn run(&mut self, bundle: &str) -> Result<BrowserTab, i32> {
        if !self.compiled.contains_key(bundle) {
            let script = NSAppleScript::initWithSource(
                NSAppleScript::alloc(),
                &NSString::from_str(&source(bundle).unwrap()),
            )
            .ok_or(0)?;
            unsafe {
                script.compileAndReturnError(None);
            }
            self.compiled.insert(bundle.into(), script);
        }
        let mut error = None;
        let result = unsafe { self.compiled[bundle].executeAndReturnError(Some(&mut error)) };
        if let Some(error) = error {
            let code = error
                .objectForKey(unsafe { NSAppleScriptErrorNumber })
                .and_then(|v| v.downcast_ref::<NSNumber>().map(|n| n.intValue()))
                .unwrap_or(0);
            Err(code)
        } else {
            Ok(decode(&result))
        }
    }
}
#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
