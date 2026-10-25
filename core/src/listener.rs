//! Loopback HTTP listener for the browser extension (`POST /v1/web`,
//! `GET` or `POST /v1/health`). A small HTTP/1.1 server on `std::net`: one request per
//! connection, read under a deadline, so a client that stalls can hold a slot
//! for at most `REQUEST_DEADLINE`.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Inner, SpentDayError, WebPage, now_ms};

/// Connections served at once. Each runs on its own thread. Past this, a new
/// connection gets 503 before anything of it is read.
const MAX_CONNECTIONS: usize = 16;
/// Time a client has to send its whole request, head and body.
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 64 * 1024;

#[derive(Deserialize)]
struct WebReport {
    browser: String,
    /// The page's origin or URL. `null` means the tab shows no web page.
    url: Option<String>,
}

/// `/v1/health`, so the extension popup can test the connection. The popup uses POST, and GET works too.
#[derive(Serialize)]
struct Health<'a> {
    ok: bool,
    device: &'a str,
}

pub fn spawn(inner: Arc<Inner>, port: u16) -> Result<(), SpentDayError> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| SpentDayError::Network(format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("spentday-web-listener".into())
        .spawn(move || {
            for stream in listener.incoming() {
                // Accept fails when the process is out of file descriptors.
                // Waiting a little keeps that from spinning a core.
                let Ok(mut stream) = stream else {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                let Some(slot) = Slot::take(&open) else {
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                    let _ = respond(&mut stream, 503, &[], "");
                    continue;
                };
                let inner = inner.clone();
                // A thread that fails to start drops its closure, and with it the slot.
                let _ = std::thread::Builder::new()
                    .name("spentday-web-request".into())
                    .spawn(move || {
                        let _slot = slot;
                        serve(&inner, stream, port);
                    });
            }
        })
        .map_err(|e| SpentDayError::Network(e.to_string()))?;
    Ok(())
}

/// One of the `MAX_CONNECTIONS` slots, given back when dropped, even when the
/// request's thread panics.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(open: &Arc<AtomicUsize>) -> Option<Slot> {
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(open.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Request {
    method: String,
    path: String,
    host: Option<String>,
    origin: Option<String>,
    body: Vec<u8>,
}

/// Why a request was not read in full.
enum Refused {
    /// The client went away or ran out of time. Nothing is sent.
    Gone,
    Status(u16),
}

/// Reads into `buf` until `deadline`. `Ok(0)` is end of stream.
fn read_until(stream: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> Result<usize, Refused> {
    let left = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(Refused::Gone)?;
    stream.set_read_timeout(Some(left)).map_err(|_| Refused::Gone)?;
    loop {
        match stream.read(buf) {
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(Refused::Gone),
            Ok(n) => return Ok(n),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, Refused> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    // The head, with its blank line, must fit in `MAX_HEAD`.
    let head_end = loop {
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            if i + 4 > MAX_HEAD {
                return Err(Refused::Status(431));
            }
            break i;
        }
        if data.len() >= MAX_HEAD {
            return Err(Refused::Status(431));
        }
        match read_until(stream, &mut chunk, deadline)? {
            0 => return Err(Refused::Gone),
            n => data.extend_from_slice(&chunk[..n]),
        }
    };
    let head = std::str::from_utf8(&data[..head_end]).map_err(|_| Refused::Status(400))?;
    // A bare CR or LF, or any other control character but tab, could end up
    // in a header this listener writes back, such as the allowed origin.
    if head
        .split("\r\n")
        .any(|line| line.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7f))
    {
        return Err(Refused::Status(400));
    }
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refused::Status(400));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(Refused::Status(400));
    }
    let mut request = Request {
        method: method.to_owned(),
        path: target.split('?').next().unwrap_or_default().to_owned(),
        host: None,
        origin: None,
        body: Vec::new(),
    };
    let mut length = None;
    // A second Host, Origin or Content-Length is ambiguous, so it is refused
    // rather than letting one of them win.
    let once = |slot: &mut Option<String>, value: &str| match slot {
        Some(_) => Err(Refused::Status(400)),
        None => {
            *slot = Some(value.to_owned());
            Ok(())
        }
    };
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(Refused::Status(400))?;
        if name.is_empty() || name.bytes().any(|b| b == b' ' || b == b'\t') {
            return Err(Refused::Status(400));
        }
        let value = value.trim_matches([' ', '\t']);
        match name.to_ascii_lowercase().as_str() {
            "host" => once(&mut request.host, value)?,
            "origin" => once(&mut request.origin, value)?,
            "content-length" => once(&mut length, value)?,
            "transfer-encoding" => return Err(Refused::Status(411)),
            _ => {}
        }
    }
    // Any run of digits, judged by its value: leading zeros are skipped, and
    // more digits than any body this listener takes is too large, not broken.
    let length = match length {
        None => 0,
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            let value = digits.trim_start_matches('0');
            if value.len() > MAX_BODY.to_string().len() {
                return Err(Refused::Status(413));
            }
            value.parse().unwrap_or(0)
        }
        Some(_) => return Err(Refused::Status(400)),
    };
    if length > MAX_BODY {
        return Err(Refused::Status(413));
    }
    let mut body = data.split_off(head_end + 4);
    while body.len() < length {
        match read_until(stream, &mut chunk, deadline)? {
            0 => return Err(Refused::Gone),
            n => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    request.body = body;
    Ok(request)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "Service Unavailable",
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    stream.write_all(out.as_bytes())?;
    stream.flush()
}

/// The `Host` a browser sends for this listener. Anything else is a page
/// that rebound its own domain to 127.0.0.1.
fn is_local_host(host: &str, port: u16) -> bool {
    let host = host.to_ascii_lowercase();
    ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|name| host == format!("{name}:{port}"))
}

fn is_extension_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://")
}

fn serve(inner: &Inner, mut stream: TcpStream, port: u16) {
    let _ = stream.set_write_timeout(Some(REQUEST_DEADLINE));
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(Refused::Status(status)) => {
            let _ = respond(&mut stream, status, &[], "");
            return;
        }
        Err(Refused::Gone) => return,
    };
    // Only the extension may talk to the listener. Web pages always send
    // their own Origin, and a page that rebinds its domain sends its own Host.
    let origin = match (&request.host, &request.origin) {
        (Some(host), Some(origin)) if is_local_host(host, port) && is_extension_origin(origin) => {
            origin.clone()
        }
        _ => {
            let _ = respond(&mut stream, 403, &[], "");
            return;
        }
    };
    let cors = [
        ("Access-Control-Allow-Origin", origin.as_str()),
        ("Vary", "Origin"),
    ];
    let methods = match request.path.as_str() {
        "/v1/web" => "POST, OPTIONS",
        "/v1/health" => "GET, POST, OPTIONS",
        _ => {
            let _ = respond(&mut stream, 404, &cors, "");
            return;
        }
    };
    let _ = match (request.method.as_str(), request.path.as_str()) {
        ("OPTIONS", _) => {
            let headers = [
                cors[0],
                cors[1],
                ("Access-Control-Allow-Methods", methods),
                ("Access-Control-Allow-Headers", "content-type"),
                ("Access-Control-Allow-Private-Network", "true"),
                ("Access-Control-Max-Age", "86400"),
            ];
            respond(&mut stream, 204, &headers, "")
        }
        // The popup asks with POST, which every browser sends with an Origin.
        ("GET" | "POST", "/v1/health") => {
            let body = serde_json::to_string(&Health {
                ok: true,
                device: &inner.device_name(),
            })
            .expect("serializable");
            let headers = [cors[0], cors[1], ("Content-Type", "application/json")];
            respond(&mut stream, 200, &headers, &body)
        }
        ("POST", "/v1/web") => {
            let status = match serde_json::from_slice::<WebReport>(&request.body) {
                Ok(report) => {
                    let page = report.url.map(|url| WebPage { url });
                    match inner.web_hint(now_ms(), &report.browser, page.as_ref()) {
                        Ok(()) => 204,
                        Err(_) => 500,
                    }
                }
                Err(_) => 400,
            };
            respond(&mut stream, status, &cors, "")
        }
        _ => respond(&mut stream, 405, &cors, ""),
    };
}
