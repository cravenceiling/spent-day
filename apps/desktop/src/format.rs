use spentday_core::SpentDayError;

/// `0m`, `<1m`, `45m`, `2h`, `5h 12m`, rounded to the nearest minute.
pub fn duration(ms: i64) -> String {
    let minutes = (ms + 30_000) / 60_000;
    if ms > 0 && minutes == 0 {
        return "<1m".into();
    }
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// A fixed message for the UI, so a server cannot put its own text there. The details go to
/// stderr, escaped. A server's error body, and network errors that may quote the server, are left
/// out of them too.
pub fn error(context: &str, e: &SpentDayError) -> String {
    eprintln!("spentday: {context}: {}", log_message(e).escape_debug());
    user_message(e)
}

pub fn log_message(e: &SpentDayError) -> String {
    match e {
        SpentDayError::Server { status, .. } => format!("the sync server answered {status}"),
        SpentDayError::Network(_) => "couldn't reach the sync server or read its answer".into(),
        _ => format!("{e:?}"),
    }
}

pub fn user_message(e: &SpentDayError) -> String {
    match e {
        SpentDayError::Storage(detail) => format!("Spent Day couldn't read its database. {detail}"),
        SpentDayError::SyncNotConfigured => format!("Sync isn't set up on this {} yet.", crate::platform::DEVICE_NOUN),
        SpentDayError::InvalidSyncCode => "That sync code isn't valid. Copy it again from a device that already syncs.".into(),
        SpentDayError::Network(_) => "Couldn't reach the sync server or read its answer. Check the address and your connection.".into(),
        SpentDayError::Server { status: 300..=399, .. } => "The sync server sent Spent Day to another address. Spent Day doesn't follow redirects, so enter the address of the sync server itself.".into(),
        SpentDayError::Server { status: 401 | 403, .. } => "The sync server didn't accept the sync code.".into(),
        SpentDayError::Server { status: 400..=499, .. } => "The sync server turned down the request.".into(),
        SpentDayError::Server { status: 500..=599, .. } => "The sync server had a problem. Try again later.".into(),
        SpentDayError::Server { .. } => "The sync server sent an answer Spent Day doesn't understand.".into(),
        SpentDayError::Decrypt => "Data on the server couldn't be decrypted. Make sure every device uses the same sync code.".into(),
        SpentDayError::InvalidArgument(detail) => format!("Spent Day couldn't use that value. {detail}"),
        SpentDayError::OldDatabase { .. } => "This data was made by an early test build of Spent Day and can't be opened. Quit Spent Day, delete the database with its -wal and -shm files, and open Spent Day again to start over.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{SpentDayError, duration, log_message, user_message};

    #[test]
    fn errors_never_show_server_text() {
        for status in [301, 302, 400, 401, 403, 404, 413, 500, 503, 600] {
            let e = SpentDayError::Server {
                status,
                message: "<b>Visit evil.example</b>".into(),
            };
            let text = user_message(&e);
            assert!(!text.contains("evil"), "{status}: {text}");
            assert!(!text.contains(&status.to_string()), "{status}: {text}");
            assert!(!log_message(&e).contains("evil"));
        }
        assert_eq!(
            user_message(&SpentDayError::Server {
                status: 401,
                message: "evil".into()
            }),
            "The sync server didn't accept the sync code."
        );
        assert!(
            user_message(&SpentDayError::Server {
                status: 302,
                message: "evil".into()
            })
            .contains("doesn't follow redirects")
        );
        let network = SpentDayError::Network("bad response: evil.example".into());
        assert!(!user_message(&network).contains("evil"));
        assert!(!log_message(&network).contains("evil"));
        assert_eq!(
            user_message(&SpentDayError::SyncNotConfigured),
            format!(
                "Sync isn't set up on this {} yet.",
                crate::platform::DEVICE_NOUN
            )
        );
    }

    #[test]
    fn durations() {
        assert_eq!(duration(0), "0m");
        assert_eq!(duration(12_000), "<1m");
        assert_eq!(duration(45 * 60_000 + 20_000), "45m");
        assert_eq!(duration(119 * 60_000 + 40_000), "2h");
        assert_eq!(duration(5 * 3_600_000 + 12 * 60_000), "5h 12m");
        assert_eq!(duration(30 * 3_600_000), "30h");
    }
}
