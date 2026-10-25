//! Web attribution: storing hints, finding the site a browser showed at a time,
//! and splitting browser intervals at site changes. Only domains are kept: a
//! URL is cut to its domain here, at the boundary, and never stored.

use crate::store::{Hint, Store};
use crate::{SpentDayError, Millis, WebPage, apps, now_ms};

/// A heartbeat only trusts a hint this young.
pub const HINT_TTL_MS: Millis = 60_000;
/// An interval's first piece only trusts a hint from before it this young.
const FIRST_PIECE_HINT_MS: Millis = 30 * 60_000;
/// Hints older than this are dropped on open and on every new hint. Android
/// ingests UsageStats every 15 minutes, so a week leaves plenty of slack for a
/// phone that was off.
const HINT_RETENTION_MS: Millis = 7 * 24 * 3_600_000;

/// Longest stored app id or app name.
pub const MAX_TEXT_BYTES: usize = 512;
/// Longest domain DNS allows. A longer host is not a site anyone visits.
const MAX_DOMAIN_BYTES: usize = 253;
/// Longest stored device name. It rides along in every pushed batch.
pub const MAX_DEVICE_NAME_BYTES: usize = 256;

/// `s` cut to at most `max` bytes on a char boundary.
pub fn clip(mut s: String, max: usize) -> String {
    if s.len() > max {
        let end = (0..=max)
            .rev()
            .find(|&i| s.is_char_boundary(i))
            .unwrap_or(0);
        s.truncate(end);
    }
    s
}

/// URL host with `www.`, `m.` and `mobile.` stripped. `None` for anything that is
/// not an http(s) page.
///
/// A scheme comes first. `http:` and `https:` input is parsed as a URL, as a
/// browser would (`http:12345` is the address 0.0.48.57). A scheme in
/// `NOT_WEB`, such as `mailto:`, `data:` or `file:`, is no page, whatever
/// follows it, so `mailto:12345` never becomes the domain `mailto`.
///
/// Input with no scheme, as Android URL bars show it, is read as https only in
/// this form: a host, then optionally `:` and a port of digits, then the end or
/// `/`, `?` or `#` and anything after. The host is `localhost`, an IPv4
/// address, a bracketed IPv6 address, or a name with at least one dot whose
/// labels are letters, digits, `-` and `_`. So `youtube.com/watch`,
/// `localhost:8080`, `[::1]:8080/x` and `example.com:000080` are pages, and
/// `cats`, `javascript:0/x` and `user@host.example` are not.
pub fn normalize_domain(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let scheme = scheme_of(raw).map(|s| s.to_ascii_lowercase());
    let parsed = match scheme.as_deref() {
        Some("http" | "https") => url::Url::parse(raw).ok()?,
        Some(other) if NOT_WEB.contains(&other) => return None,
        _ if is_schemeless_page(raw) => url::Url::parse(&format!("https://{raw}")).ok()?,
        _ => return None,
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let host = ["www.", "m.", "mobile."]
        .iter()
        .find_map(|prefix| host.strip_prefix(prefix))
        .unwrap_or(&host);
    if host.is_empty() || host.len() > MAX_DOMAIN_BYTES {
        None
    } else {
        Some(host.to_owned())
    }
}

/// Schemes that never name a web page. One of these before a colon wins over
/// reading the input as `host:port`.
const NOT_WEB: &[&str] = &[
    "about", "blob", "brave", "chrome", "chrome-extension", "content", "data", "edge",
    "file", "filesystem", "ftp", "geo", "intent", "javascript", "magnet", "mailto", "market",
    "moz-extension", "opera", "sms", "ssh", "tel", "view-source", "vivaldi", "ws", "wss",
];

/// The text before the first colon, when it has the shape of a scheme.
fn scheme_of(raw: &str) -> Option<&str> {
    let (scheme, _) = raw.split_once(':')?;
    let mut chars = scheme.chars();
    (chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

/// Whether `raw` has the schemeless form `normalize_domain` describes.
fn is_schemeless_page(raw: &str) -> bool {
    let end = raw.find(['/', '?', '#']).unwrap_or(raw.len());
    let authority = &raw[..end];
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((ip, after)) if ip.parse::<std::net::Ipv6Addr>().is_ok() => match after {
                "" => return true,
                after => ("[]", after.strip_prefix(':')),
            },
            _ => return false,
        },
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    if let Some(port) = port
        && (port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    if host == "[]" {
        return port.is_some();
    }
    let name = host.strip_suffix('.').unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok()
        || (name.contains('.')
            && name.split('.').all(|label| {
                !label.is_empty()
                    && label.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
            }))
}

/// Key under which hints for this browser are stored and looked up.
fn hint_browser(browser: &str) -> (String, bool) {
    match apps::browser_key(browser) {
        Some(key) => (key.to_owned(), true),
        None => (browser.trim().to_ascii_lowercase(), false),
    }
}

/// Hints stamped further ahead of the system clock than this are dropped: a
/// phone with a wrong date would otherwise outlive every real hint.
const MAX_HINT_LEAD_MS: Millis = 24 * 3_600_000;

/// Stores a hint as its domain. `None` is a no-page hint: the browser shows no
/// web page (new tab, `chrome://`, a PDF, a private window), so no site gets
/// its time. A page that is not http(s) is ignored.
pub fn record_hint(
    store: &Store,
    at_ms: Millis,
    browser: &str,
    page: Option<&WebPage>,
) -> Result<(), SpentDayError> {
    let domain = match page {
        Some(page) => match normalize_domain(&page.url) {
            Some(domain) => domain,
            None => return Ok(()),
        },
        None => String::new(),
    };
    if at_ms > now_ms().saturating_add(MAX_HINT_LEAD_MS) {
        return Ok(());
    }
    let (key, known) = hint_browser(browser);
    store.add_hint(at_ms, &key, known, &domain)?;
    prune(store)
}

/// Measured from the newest hint, or from now when that is earlier, so a
/// device with a clock behind, or a script replaying old timestamps, keeps the
/// hints it needs, and a hint from a clock ahead cannot push the cutoff past
/// real hints.
pub fn prune(store: &Store) -> Result<(), SpentDayError> {
    store.prune_hints(HINT_RETENTION_MS, now_ms())
}

/// The domain browser `app_id` showed at heartbeat `at_ms`: the latest hint
/// at most `HINT_TTL_MS` old that names this browser, or a browser the app
/// table does not know (an extension in an unlisted Chromium fork).
pub fn domain_at(store: &Store, app_id: &str, at_ms: Millis) -> Result<Option<String>, SpentDayError> {
    let Some(key) = browser_of(app_id) else {
        return Ok(None);
    };
    let hint = store.latest_hint(key, true, at_ms.saturating_sub(HINT_TTL_MS), at_ms)?;
    Ok(hint.and_then(domain))
}

fn browser_of(app_id: &str) -> Option<&'static str> {
    apps::lookup(app_id)
        .filter(|app| app.browser)
        .map(|app| app.key)
}

/// `None` for a no-page hint, stored with an empty domain.
fn domain(hint: Hint) -> Option<String> {
    (!hint.domain.is_empty()).then_some(hint.domain)
}

/// One piece of an interval after splitting at hints.
pub struct Piece {
    pub start_ms: Millis,
    pub end_ms: Millis,
    pub domain: Option<String>,
}

/// Splits a browser interval at every hint for that browser inside it. The
/// first piece takes the latest hint at or before `start_ms` if it is at most
/// `FIRST_PIECE_HINT_MS` old: the page was still in front when the user
/// switched back, unless they spent a long time elsewhere.
pub fn split_interval(
    store: &Store,
    app_id: &str,
    start_ms: Millis,
    end_ms: Millis,
) -> Result<Vec<Piece>, SpentDayError> {
    let Some(key) = browser_of(app_id) else {
        return Ok(vec![Piece {
            start_ms,
            end_ms,
            domain: None,
        }]);
    };
    let mut pieces = vec![Piece {
        start_ms,
        end_ms,
        domain: store
            .latest_hint(
                key,
                false,
                start_ms.saturating_sub(FIRST_PIECE_HINT_MS),
                start_ms,
            )?
            .and_then(domain),
    }];
    for hint in store.hints_inside(key, start_ms, end_ms)? {
        let last = pieces.last_mut().expect("starts non-empty");
        if last.start_ms == hint.at_ms {
            last.domain = domain(hint);
            continue;
        }
        last.end_ms = hint.at_ms;
        pieces.push(Piece {
            start_ms: hint.at_ms,
            end_ms,
            domain: domain(hint),
        });
    }
    // The extension re-sends the open page every 10 s. A hint that keeps the
    // domain is not a site change, so neighbours on one domain become one piece.
    pieces.dedup_by(|next, kept| {
        let same = next.domain == kept.domain;
        if same {
            kept.end_ms = next.end_ms;
        }
        same
    });
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::normalize_domain;

    #[test]
    fn normalizes_domains() {
        assert_eq!(
            normalize_domain("https://www.YouTube.com/watch?v=1").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            normalize_domain("http://m.youtube.com:8080/x").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            normalize_domain("https://mobile.twitter.com/home").as_deref(),
            Some("twitter.com")
        );
        assert_eq!(
            normalize_domain("en.m.wikipedia.org/wiki/Rust").as_deref(),
            Some("en.m.wikipedia.org")
        );
        assert_eq!(
            normalize_domain("github.com/rust-lang").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            normalize_domain("https://docs.rs.").as_deref(),
            Some("docs.rs")
        );
        assert_eq!(normalize_domain("chrome://newtab"), None);
        assert_eq!(normalize_domain("about:blank"), None);
        assert_eq!(normalize_domain("file:///etc/hosts"), None);
        // An explicit scheme other than http(s) is no page, whatever follows it.
        for raw in [
            "data:,user@private-medical-note",
            "data:text/html,<p>secret</p>",
            "file:/private-path",
            "FILE:/private-path",
            "mailto:alice@secret-mail.example",
            "javascript:alert(1)",
            "blob:https://example.com/0f2c",
            "view-source:https://example.com/",
            "tel:+15550100",
            "ftp://files.example.com/",
        ] {
            assert_eq!(normalize_domain(raw), None, "{raw}");
        }
        // host:port and bracketed IPv6 still read as schemeless pages.
        assert_eq!(normalize_domain("localhost:8080").as_deref(), Some("localhost"));
        assert_eq!(
            normalize_domain("example.com:8080/path?q=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(normalize_domain("[::1]:8080/x").as_deref(), Some("[::1]"));
        assert_eq!(normalize_domain("192.168.1.2:80").as_deref(), Some("192.168.1.2"));
        assert_eq!(normalize_domain("HTTPS://Example.com/").as_deref(), Some("example.com"));
        assert_eq!(normalize_domain("http:example.com").as_deref(), Some("example.com"));
        // A recognized scheme wins over reading `scheme:digits` as host:port.
        for raw in ["mailto:12345", "tel:12345", "data:12345", "file:12345", "javascript:0/x", "about:0"] {
            assert_eq!(normalize_domain(raw), None, "{raw}");
        }
        // http: input is a URL, read as a browser reads it, never the domain `http`.
        assert_eq!(normalize_domain("http:12345").as_deref(), Some("0.0.48.57"));
        assert_eq!(normalize_domain("https:12345").as_deref(), Some("0.0.48.57"));
        // The schemeless form: a port may have leading zeros.
        assert_eq!(normalize_domain("example.com:000080").as_deref(), Some("example.com"));
        assert_eq!(normalize_domain("[::1]").as_deref(), Some("[::1]"));
        assert_eq!(normalize_domain("10.0.0.2/x").as_deref(), Some("10.0.0.2"));
        assert_eq!(normalize_domain("bücher.example/").as_deref(), Some("xn--bcher-kva.example"));
        // Anything else without a scheme is no page: a search term, a single
        // label, user info, an empty or odd port, an unbracketed IPv6 address.
        for raw in [
            "cats",
            "cats and dogs",
            "intranet/page",
            "user@host.example",
            "example.com:",
            "example.com:80x",
            "example..com",
            "::1",
            "[::1]:",
            "[nonsense]:80",
            "unknownscheme:x",
        ] {
            assert_eq!(normalize_domain(raw), None, "{raw}");
        }
        assert_eq!(normalize_domain(""), None);
    }
}
