use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSDictionary, NSNumber, NSString};

pub fn dictionary(value: &CFDictionary) -> &NSDictionary {
    // These dictionaries come from Copy APIs with CFType callbacks and Foundation-compatible values.
    unsafe { &*(value as *const CFDictionary).cast::<NSDictionary>() }
}

pub fn number(value: &NSDictionary, key: &str) -> Option<i64> {
    value
        .objectForKey(&NSString::from_str(key))?
        .downcast_ref::<NSNumber>()
        .map(|v| v.longLongValue())
}
