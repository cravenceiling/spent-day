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
    match e {
        SpentDayError::Server { status, .. } => {
            eprintln!("spentday: {context}: the server answered {status}")
        }
        // Its text can quote the server's answer.
        SpentDayError::Network(_) => {
            eprintln!("spentday: {context}: could not reach the server or read its answer")
        }
        _ => eprintln!("spentday: {context}: {e:?}"),
    }
    error_text(e)
}

fn error_text(e: &SpentDayError) -> String {
    match e {
        SpentDayError::Server { status: 401 | 403, .. } => "The server did not accept the sync code".into(),
        SpentDayError::Server { status: 400..=499, .. } => "The server turned down the request".into(),
        SpentDayError::Server { status: 500..=599, .. } => "The server had a problem. Spent Day tries again later.".into(),
        SpentDayError::Server { .. } => "The server sent an answer Spent Day does not understand".into(),
        SpentDayError::Network(_) => "Could not reach the server or read its answer".into(),
        SpentDayError::Decrypt => "Could not decrypt data from the server. Check the sync code.".into(),
        SpentDayError::InvalidSyncCode => "That sync code is not valid".into(),
        SpentDayError::SyncNotConfigured => "Sync is not set up".into(),
        SpentDayError::Storage(_) => "Could not read or write the database".into(),
        SpentDayError::OldDatabase { .. } => "This data was made by an early test build of Spent Day and cannot be opened. Delete the database to start over.".into(),
        // Made on this device from the server address the user typed, never from a server's answer.
        SpentDayError::InvalidArgument(why) => why.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{SpentDayError, duration, error_text};

    #[test]
    fn errors_never_show_server_text() {
        let server = |status| SpentDayError::Server {
            status,
            message: "<b>Visit evil.example</b>".into(),
        };
        for status in [301, 400, 401, 403, 404, 413, 500, 503] {
            let text = error_text(&server(status));
            assert!(!text.contains("evil"), "{status}: {text}");
            assert!(!text.contains(&status.to_string()), "{status}: {text}");
        }
        assert_eq!(
            error_text(&server(401)),
            "The server did not accept the sync code"
        );
        assert_eq!(
            error_text(&server(404)),
            "The server turned down the request"
        );
        assert_eq!(
            error_text(&server(502)),
            "The server had a problem. Spent Day tries again later."
        );
        let network = SpentDayError::Network(
            "bad response from server: invalid type: string \"evil\"".into(),
        );
        assert_eq!(
            error_text(&network),
            "Could not reach the server or read its answer"
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
