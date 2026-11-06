use super::{
    browser::{self, BrowserPages, BrowserTab},
    front::FrontApp,
};
use spentday_core::{Activity, SpentDay, SpentDayError, WebPage};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Recorder {
    core: Arc<SpentDay>,
    browsers: BrowserPages,
    activation: u64,
    last_tabs: HashMap<String, BrowserTab>,
}
impl Recorder {
    pub fn new(core: Arc<SpentDay>, browsers: BrowserPages) -> Self {
        Self {
            core,
            browsers,
            activation: 0,
            last_tabs: HashMap::new(),
        }
    }
    pub fn activated(&mut self) {
        self.activation += 1;
    }
    pub fn record(
        &mut self,
        front: Option<FrontApp>,
        now: i64,
        safari: bool,
    ) -> Result<(), SpentDayError> {
        let Some(front) = front else {
            return self.core.heartbeat(now, None);
        };
        let mut web = None;
        if browser::source(&front.id).is_some() {
            let tab = if browser::is_safari(&front.id) && !safari {
                Some(BrowserTab::NoPage)
            } else {
                self.browsers.tab(&front.id, front.pid, self.activation)
            };
            if tab == Some(BrowserTab::NoPage)
                && self.last_tabs.get(&front.id) != Some(&BrowserTab::NoPage)
            {
                self.core.web_cleared(now, front.id.clone())?;
            }
            if let Some(BrowserTab::Page(ref url)) = tab {
                web = Some(WebPage { url: url.clone() });
            }
            if let Some(tab) = tab {
                self.last_tabs.insert(front.id.clone(), tab);
            } else {
                self.last_tabs.remove(&front.id);
            }
        }
        self.core.heartbeat(
            now,
            Some(Activity {
                app_id: front.id,
                app_name: front.name,
                web,
            }),
        )
    }
}
