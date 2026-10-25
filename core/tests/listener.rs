use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use spentday_core::{Activity, Platform, SpentDay};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn listening(dir: &tempfile::TempDir, name: &str) -> (Arc<SpentDay>, u16) {
    let m = SpentDay::open(
        dir.path().join("m.db").to_string_lossy().into_owned(),
        name.into(),
        Platform::Linux,
    )
    .unwrap();
    let port = free_port();
    m.start_web_listener(port).unwrap();
    (m, port)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into()
}

/// Sends one raw request and returns the status code, or `None` when the
/// listener closed without answering.
fn raw(port: u16, request: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer.split(' ').nth(1).and_then(|code| code.parse().ok())
}

const CHROME: &str = "chrome-extension://abcdef";

#[test]
fn extension_reports_become_hints_and_other_origins_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (m, port) = listening(&dir, "box");
    m.start_web_listener(port).unwrap();

    let agent = agent();
    let url = format!("http://127.0.0.1:{port}/v1/web");
    let post = |origin: Option<&str>, body: &str| {
        let mut req = agent.post(&url).header("Content-Type", "application/json");
        if let Some(o) = origin {
            req = req.header("Origin", o);
        }
        let res = req.send(body.to_owned()).unwrap();
        let allow = res
            .headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap().to_owned());
        (res.status().as_u16(), allow)
    };

    let preflight = agent
        .options(&url)
        .header("Origin", "moz-extension://abc")
        .header("Access-Control-Request-Method", "POST")
        .call()
        .unwrap();
    assert_eq!(preflight.status().as_u16(), 204);
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "moz-extension://abc"
    );
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-methods")
            .unwrap(),
        "POST, OPTIONS"
    );
    let refused = agent
        .options(&url)
        .header("Origin", "https://evil.example")
        .call()
        .unwrap();
    assert_eq!(refused.status().as_u16(), 403);

    let report = r#"{"browser":"chrome","url":"https://www.youtube.com"}"#;
    assert_eq!(post(Some(CHROME), report), (204, Some(CHROME.to_owned())));
    assert_eq!(
        post(
            Some("https://evil.example"),
            r#"{"browser":"chrome","url":"https://evil.example/"}"#
        ),
        (403, None)
    );
    // Without an Origin it is not the extension.
    assert_eq!(
        post(None, r#"{"browser":"chrome","url":"https://evil.example/"}"#),
        (403, None)
    );
    assert_eq!(
        post(Some(CHROME), r#"{"browser":"chrome","url":"chrome://settings"}"#).0,
        204
    );
    assert_eq!(post(Some(CHROME), "not json").0, 400);

    let at = now_ms();
    let browser = Activity {
        app_id: "com.google.Chrome".into(),
        app_name: "Chrome".into(),
        web: None,
    };
    m.heartbeat(at, Some(browser.clone())).unwrap();
    m.heartbeat(at + 5_000, Some(browser.clone())).unwrap();
    let span = &m.recent_spans(1).unwrap()[0];
    assert_eq!(span.domain.as_deref(), Some("youtube.com"));

    // `url: null` is a no-page hint: the next heartbeat has no site.
    assert_eq!(post(Some(CHROME), r#"{"browser":"chrome","url":null}"#).0, 204);
    m.heartbeat(now_ms() + 10_000, Some(browser)).unwrap();
    let span = &m.recent_spans(1).unwrap()[0];
    assert_eq!(span.domain, None);
}

/// A page that rebinds its own domain to 127.0.0.1 reaches the listener
/// with its own name in `Host`.
#[test]
fn requests_for_another_host_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "box");
    let health = |host: &str, origin: &str| {
        raw(
            port,
            &format!("GET /v1/health HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\n\r\n"),
        )
    };
    assert_eq!(health(&format!("127.0.0.1:{port}"), CHROME), Some(200));
    assert_eq!(health(&format!("localhost:{port}"), CHROME), Some(200));
    assert_eq!(health(&format!("[::1]:{port}"), CHROME), Some(200));
    assert_eq!(health(&format!("rebind.example:{port}"), CHROME), Some(403));
    assert_eq!(health("127.0.0.1", CHROME), Some(403));
    assert_eq!(health(&format!("127.0.0.1:{}", port + 1), CHROME), Some(403));
    assert_eq!(
        raw(port, "GET /v1/health HTTP/1.1\r\nHost: rebind.example\r\n\r\n"),
        Some(403)
    );
    assert_eq!(
        raw(port, &format!("GET /v1/health HTTP/1.1\r\nOrigin: {CHROME}\r\n\r\n")),
        Some(403)
    );
}

fn stall(port: u16) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            format!(
                "POST /v1/web HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {CHROME}\r\n\
                 Content-Type: application/json\r\nContent-Length: 4096\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    stream
}

fn health(port: u16) -> Option<u16> {
    raw(
        port,
        &format!("GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {CHROME}\r\n\r\n"),
    )
}

#[test]
fn clients_that_never_send_their_body_do_not_block_others() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "box");

    // Eight withheld uploads used to take every slot until they closed.
    let eight: Vec<_> = (0..8).map(|_| stall(port)).collect();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(health(port), Some(200));

    // Enough to fill every slot gets a 503 at most until their deadline.
    let more: Vec<_> = (0..16).map(|_| stall(port)).collect();
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    while health(port) != Some(200) {
        assert!(started.elapsed() < Duration::from_secs(5), "still busy");
        std::thread::sleep(Duration::from_millis(200));
    }
    drop((eight, more));
}

#[test]
fn a_slow_client_still_gets_its_answer_within_the_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "box");
    let body = r#"{"browser":"firefox","url":"https://example.com"}"#;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request = format!(
        "POST /v1/web HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: moz-extension://x\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    for piece in request.as_bytes().chunks(16) {
        stream.write_all(piece).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 204 "), "{answer}");
}

#[test]
fn oversized_and_chunked_bodies_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "box");
    let head = format!("POST /v1/web HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {CHROME}\r\n");
    assert_eq!(
        raw(port, &format!("{head}Content-Length: 100000\r\n\r\n")),
        Some(413)
    );
    assert_eq!(
        raw(port, &format!("{head}Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n")),
        Some(411)
    );
    let huge = format!("{head}X-Pad: {}\r\n\r\n", "a".repeat(20_000));
    assert_eq!(raw(port, &huge), Some(431));
}

#[test]
fn health_names_the_device_under_the_same_origin_rules() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "Work \"Mac\"");
    let agent = agent();
    let url = format!("http://127.0.0.1:{port}/v1/health");
    let get = |origin: Option<&str>| {
        let mut req = agent.get(&url);
        if let Some(o) = origin {
            req = req.header("Origin", o);
        }
        let mut res = req.call().unwrap();
        let header = |name: &str| {
            res.headers()
                .get(name)
                .map(|v| v.to_str().unwrap().to_owned())
        };
        let (allow, kind) = (
            header("access-control-allow-origin"),
            header("content-type"),
        );
        let body = res.body_mut().read_to_string().unwrap();
        (res.status().as_u16(), allow, kind, body)
    };

    assert_eq!(
        get(Some(CHROME)),
        (
            200,
            Some(CHROME.to_owned()),
            Some("application/json".to_owned()),
            r#"{"ok":true,"device":"Work \"Mac\""}"#.to_owned()
        )
    );
    assert_eq!(get(None).0, 403);
    assert_eq!(get(Some("https://evil.example")).0, 403);

    let preflight = agent
        .options(&url)
        .header("Origin", "moz-extension://abc")
        .header("Access-Control-Request-Method", "GET")
        .call()
        .unwrap();
    assert_eq!(preflight.status().as_u16(), 204);
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-methods")
            .unwrap(),
        "GET, POST, OPTIONS"
    );
    // The popup asks with POST, so the browser always sends its Origin.
    let mut post = agent
        .post(&url)
        .header("Origin", "moz-extension://abc")
        .send("")
        .unwrap();
    assert_eq!(post.status().as_u16(), 200);
    assert_eq!(
        post.body_mut().read_to_string().unwrap(),
        r#"{"ok":true,"device":"Work \"Mac\""}"#
    );
    let put = agent
        .put(&url)
        .header("Origin", "moz-extension://abc")
        .send("{}")
        .unwrap();
    assert_eq!(put.status().as_u16(), 405);
}

#[test]
fn malformed_heads_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "box");
    let host = format!("Host: 127.0.0.1:{port}\r\n");
    let origin = format!("Origin: {CHROME}\r\n");
    let get = |headers: &str| raw(port, &format!("GET /v1/health HTTP/1.1\r\n{headers}\r\n"));
    let post = |headers: &str| {
        raw(
            port,
            &format!("POST /v1/web HTTP/1.1\r\n{host}{origin}{headers}\r\n{{\"browser\":\"chrome\",\"url\":null}}"),
        )
    };
    assert_eq!(get(&format!("{host}{origin}")), Some(200));

    // A bare LF or CR would carry a header of its own into the allowed origin.
    assert_eq!(get(&format!("{host}Origin: {CHROME}\nX-Injected: 1\r\n")), Some(400));
    assert_eq!(get(&format!("{host}Origin: {CHROME}\rX-Injected: 1\r\n")), Some(400));
    assert_eq!(get(&format!("{host}{origin}X-Bell: \u{7}\r\n")), Some(400));
    assert_eq!(get(&format!("{host}{origin}X-Tab: a\tb\r\n")), Some(200));
    // Folded and nameless headers.
    assert_eq!(get(&format!("{host}{origin} X-Folded: 1\r\n")), Some(400));
    assert_eq!(get(&format!("{host}{origin}: 1\r\n")), Some(400));

    // Duplicates are ambiguous, whichever one comes last.
    assert_eq!(get(&format!("{host}Host: rebind.example:{port}\r\n{origin}")), Some(400));
    assert_eq!(get(&format!("Host: rebind.example:{port}\r\n{host}{origin}")), Some(400));
    assert_eq!(get(&format!("{host}{origin}Origin: https://evil.example\r\n")), Some(400));
    assert_eq!(post("Content-Length: 31\r\nContent-Length: 31\r\n"), Some(400));

    // Content-Length is digits only, judged by its value.
    assert_eq!(post("Content-Length: 31\r\n"), Some(204));
    assert_eq!(post("Content-Length: 0000000000031\r\n"), Some(204));
    assert_eq!(get(&format!("{host}{origin}Content-Length: 0000000000\r\n")), Some(200));
    for huge in ["65537", "1000000000", "99999999999999999999999999999"] {
        assert_eq!(post(&format!("Content-Length: {huge}\r\n")), Some(413), "{huge}");
    }
    for bad in ["+31", "-1", "0x1f", "31 31", "", "3_1"] {
        assert_eq!(post(&format!("Content-Length: {bad}\r\n")), Some(400), "{bad:?}");
    }
}

/// The head, with its blank line, may be 8 KiB and no more, however the
/// bytes arrive.
#[test]
fn a_head_just_over_the_limit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_m, port) = listening(&dir, "box");
    let head = |len: usize| {
        let start = format!("GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {CHROME}\r\nX-Pad: ");
        let pad = len - start.len() - "\r\n\r\n".len();
        format!("{start}{}\r\n\r\n", "a".repeat(pad))
    };
    assert_eq!(raw(port, &head(8_192)), Some(200));
    assert_eq!(raw(port, &head(8_193)), Some(431));
    assert_eq!(raw(port, &head(8_590)), Some(431));
}
