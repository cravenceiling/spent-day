use std::collections::HashMap;
use std::ptr::NonNull;

use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use objc2_io_kit::IOPMCopyAssertionsByProcess;

use super::native::{dictionary, number};

#[derive(Clone, Debug)]
pub struct Assertion {
    pub kind: String,
    pub on_behalf_of: Option<i32>,
}
pub type Assertions = HashMap<i32, Vec<Assertion>>;

pub fn last_input_ms(now: i64) -> i64 {
    let idle = CGEventSource::seconds_since_last_event_type(
        CGEventSourceStateID::CombinedSessionState,
        CGEventType(u32::MAX),
    );
    now - (idle * 1000.0) as i64
}

pub fn current_assertions() -> Assertions {
    let mut raw = std::ptr::null();
    if unsafe { IOPMCopyAssertionsByProcess(&mut raw) } != 0 {
        return HashMap::new();
    }
    let Some(raw) = NonNull::new(raw.cast_mut()) else {
        return HashMap::new();
    };
    // Copy transfers one ownership reference; CFRetained releases it after decoding.
    let owned = unsafe { CFRetained::from_raw(raw) };
    let dict = dictionary(&owned);
    let mut result = HashMap::new();
    for owner in &dict.allKeys() {
        let Some(pid) = owner.downcast_ref::<NSNumber>() else {
            continue;
        };
        let Some(list) = dict.objectForKey(&owner) else {
            continue;
        };
        let Some(list) = list.downcast_ref::<NSArray>() else {
            continue;
        };
        let mut assertions = Vec::new();
        for entry in list {
            let Some(entry) = entry.downcast_ref::<NSDictionary>() else {
                continue;
            };
            let Some(kind) = entry.objectForKey(&NSString::from_str("AssertType")) else {
                continue;
            };
            let Some(kind) = kind.downcast_ref::<NSString>() else {
                continue;
            };
            assertions.push(Assertion {
                kind: kind.to_string(),
                on_behalf_of: number(entry, "AssertionOnBehalfOfPID").map(|n| n as i32),
            });
        }
        result.insert(pid.intValue(), assertions);
    }
    result
}

pub fn holds_display_awake(pid: i32, assertions: &Assertions) -> bool {
    assertions.iter().any(|(&owner, list)| {
        list.iter().any(|a| {
            matches!(
                a.kind.as_str(),
                "PreventUserIdleDisplaySleep" | "NoDisplaySleepAssertion"
            ) && (owner == pid || a.on_behalf_of == Some(pid))
        })
    })
}

#[derive(Default)]
pub struct LastSeen {
    held_awake_at: Option<i64>,
}
impl LastSeen {
    pub fn away_since(
        &mut self,
        now: i64,
        last_input: i64,
        holds: impl FnOnce() -> bool,
    ) -> Option<i64> {
        if now - last_input <= 7_200_000 && holds() {
            self.held_awake_at = Some(now);
        }
        let seen = last_input.max(self.held_awake_at.unwrap_or(last_input));
        (now - seen > 180_000).then_some(seen)
    }
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
