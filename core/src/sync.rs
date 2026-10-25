//! Sync code, key derivation, batch encryption and the HTTP client.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store::Span;
use crate::{SpentDayError, Platform};

pub const PUSH_BATCH_SPANS: usize = 2000;
/// Plaintext budget for one pushed batch. Sealed and base64-encoded it stays
/// near 4 MiB, half the server's 8 MiB body cap.
const PUSH_BATCH_BYTES: usize = 3 * 1024 * 1024;
pub const PULL_PAGE: usize = 200;
/// The server caps a page near 4 MiB but always sends at least one batch, and
/// a pushed batch is at most 8 MiB, so no page reaches this.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

type Result<T> = std::result::Result<T, SpentDayError>;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SECRET_LEN: usize = 32;
/// 256 secret bits plus a 24-bit check (the first 3 bytes of SHA-256 of the
/// secret) fill exactly 56 base32 symbols, 14 groups of 4. A typo passes the
/// check about once in 16.7 million tries, so it fails as `InvalidSyncCode`
/// instead of silently joining a new, empty account.
const CHECK_BITS: usize = 24;
const CODE_SYMBOLS: usize = (SECRET_LEN * 8 + CHECK_BITS) / 5;

fn check_bits(secret: &[u8]) -> impl Iterator<Item = u8> {
    let digest = Sha256::digest(secret);
    (0..CHECK_BITS).map(move |i| (digest[i / 8] >> (7 - i % 8)) & 1)
}

pub fn encode_code(secret: &[u8; SECRET_LEN]) -> String {
    let mut bits: Vec<u8> = secret
        .iter()
        .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1))
        .collect();
    bits.extend(check_bits(secret));
    let symbols: Vec<char> = bits
        .chunks(5)
        .map(|c| CROCKFORD[c.iter().fold(0usize, |acc, &b| (acc << 1) | b as usize)] as char)
        .collect();
    symbols
        .chunks(4)
        .map(|g| g.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn decode_code(code: &str) -> Result<[u8; SECRET_LEN]> {
    let mut bits = Vec::with_capacity(CODE_SYMBOLS * 5);
    for c in code.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let value = CROCKFORD
            .iter()
            .position(|&s| s as char == c)
            .ok_or(SpentDayError::InvalidSyncCode)?;
        bits.extend((0..5).rev().map(|i| ((value >> i) & 1) as u8));
    }
    if bits.len() != CODE_SYMBOLS * 5 {
        return Err(SpentDayError::InvalidSyncCode);
    }
    let mut secret = [0u8; SECRET_LEN];
    for (i, byte) in secret.iter_mut().enumerate() {
        *byte = bits[i * 8..i * 8 + 8]
            .iter()
            .fold(0, |acc, &b| (acc << 1) | b);
    }
    if !bits[SECRET_LEN * 8..]
        .iter()
        .copied()
        .eq(check_bits(&secret))
    {
        return Err(SpentDayError::InvalidSyncCode);
    }
    Ok(secret)
}

pub fn new_code() -> String {
    let mut secret = [0u8; SECRET_LEN];
    rand::rng().fill_bytes(&mut secret);
    encode_code(&secret)
}

/// Canonical form of a user-typed code.
pub fn canonical_code(code: &str) -> Result<String> {
    decode_code(code).map(|s| encode_code(&s))
}

pub struct Keys {
    pub space_id: String,
    pub auth_token: String,
    enc_key: [u8; 32],
}

impl Keys {
    pub fn derive(code: &str) -> Result<Keys> {
        let secret = decode_code(code)?;
        let hk = Hkdf::<Sha256>::new(Some(b"spentday-sync-v1"), &secret);
        let expand = |info: &[u8], out: &mut [u8]| {
            hk.expand(info, out).expect("HKDF output length is valid")
        };
        let mut space = [0u8; 16];
        let mut token = [0u8; 32];
        let mut enc_key = [0u8; 32];
        expand(b"space_id", &mut space);
        expand(b"auth_token", &mut token);
        expand(b"enc_key", &mut enc_key);
        Ok(Keys {
            space_id: hex::encode(space),
            auth_token: hex::encode(token),
            enc_key,
        })
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new_from_slice(&self.enc_key).expect("32-byte key")
    }

    fn aad(&self, device_id: &str) -> Vec<u8> {
        [self.space_id.as_bytes(), device_id.as_bytes()].concat()
    }

    pub fn seal(&self, device_id: &str, payload: &impl Serialize) -> Envelope {
        let mut nonce = [0u8; 24];
        rand::rng().fill_bytes(&mut nonce);
        let plaintext = serde_json::to_vec(payload).expect("payload serializes");
        let aad = self.aad(device_id);
        let ciphertext = self
            .cipher()
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .expect("encryption does not fail");
        Envelope {
            device_id: device_id.to_owned(),
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(ciphertext),
        }
    }

    pub fn open(&self, envelope: &Envelope) -> Result<Batch> {
        let nonce: [u8; 24] = B64
            .decode(&envelope.nonce)
            .ok()
            .and_then(|n| n.try_into().ok())
            .ok_or(SpentDayError::Decrypt)?;
        let ciphertext = B64
            .decode(&envelope.ciphertext)
            .map_err(|_| SpentDayError::Decrypt)?;
        let aad = self.aad(&envelope.device_id);
        let plaintext = self
            .cipher()
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| SpentDayError::Decrypt)?;
        let batch: Batch = serde_json::from_slice(&plaintext).map_err(|_| SpentDayError::Decrypt)?;
        if batch.device.id != envelope.device_id {
            return Err(SpentDayError::Decrypt);
        }
        Ok(batch)
    }
}

/// The longest prefix of `spans` that fits `PUSH_BATCH_BYTES` beside `device`,
/// and never empty when `spans` is not: stored fields and the device name are
/// clipped, so one span always fits.
pub fn fit_batch(device: &WireDevice, mut spans: Vec<Span>) -> Vec<Span> {
    let mut bytes = serde_json::to_vec(device).map_or(0, |json| json.len());
    let fits = spans
        .iter()
        .position(|span| {
            bytes += serde_json::to_vec(span).map_or(0, |json| json.len()) + 1;
            bytes > PUSH_BATCH_BYTES
        })
        .unwrap_or(spans.len())
        .max(1);
    spans.truncate(fits);
    spans
}

#[derive(Serialize, Deserialize)]
pub struct WireDevice {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "lenient_platform")]
    pub platform: Platform,
}

fn lenient_platform<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Platform, D::Error> {
    String::deserialize(d).map(|name| crate::store::parse_platform(&name))
}

/// Plaintext of one encrypted batch.
#[derive(Serialize, Deserialize)]
pub struct Batch {
    pub device: WireDevice,
    pub spans: Vec<Span>,
}

#[derive(Serialize, Deserialize)]
pub struct Envelope {
    pub device_id: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Deserialize)]
pub struct PulledEnvelope {
    pub cursor: i64,
    #[serde(flatten)]
    pub envelope: Envelope,
}

#[derive(Deserialize)]
pub struct Page {
    pub batches: Vec<PulledEnvelope>,
    pub more: bool,
    /// The newest cursor in the space, `0` when it is empty. A stored cursor
    /// past it was poisoned and the client reads the space again from `0`.
    pub head: i64,
}

/// `https://` for any host. Plain `http://` only where traffic stays on this
/// device or a private network: loopback, `localhost` and private IP literals
/// (which include `10.0.2.2`, the Android emulator's host). Returns the URL to
/// dial, which is the one that was checked, so no resolver decides where
/// plain http goes: `10.0.0.1.` reads as the address 10.0.0.1, and plain
/// `http://localhost` becomes `http://127.0.0.1`.
pub fn check_server_url(server_url: &str) -> Result<String> {
    let invalid = |why: String| Err(SpentDayError::InvalidArgument(why));
    let Ok(mut parsed) = url::Url::parse(server_url) else {
        return invalid(format!("{server_url} is not a URL. It should look like https://sync.example.com."));
    };
    let host = match parsed.host() {
        Some(host) if matches!(parsed.scheme(), "http" | "https") => host.to_owned(),
        _ => {
            return invalid(format!(
                "{server_url} is not a server address. It should start with https://."
            ));
        }
    };
    if parsed.scheme() == "http" {
        match host {
            url::Host::Domain(name) if name.trim_end_matches('.') == "localhost" => {
                parsed
                    .set_host(Some("127.0.0.1"))
                    .expect("an IP literal is a valid host");
            }
            url::Host::Ipv4(ip) if ip.is_loopback() || ip.is_private() => {}
            url::Host::Ipv6(ip) if ipv6_is_local(ip) => {}
            _ => {
                return invalid(format!(
                    "Use https:// for {host}. Plain http:// only works for this device or a private network address."
                ));
            }
        }
    }
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

fn ipv6_is_local(ip: std::net::Ipv6Addr) -> bool {
    ip.is_loopback()
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        || ip
            .to_ipv4_mapped()
            .is_some_and(|v4| v4.is_loopback() || v4.is_private())
}

#[derive(Deserialize)]
struct PushResponse {
    #[allow(dead_code)]
    cursor: i64,
}

pub struct Client {
    agent: ureq::Agent,
    base: String,
    keys: Keys,
}

impl Client {
    pub fn new(server_url: &str, keys: Keys) -> Client {
        // The API never redirects. Following one would let a server move
        // the client to plain http, or to an address the https rule refuses.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Client {
            agent,
            base: server_url.trim_end_matches('/').to_owned(),
            keys,
        }
    }

    /// The server URL this client dials.
    #[cfg(test)]
    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    fn batches_url(&self) -> String {
        format!("{}/v1/spaces/{}/batches", self.base, self.keys.space_id)
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.keys.auth_token)
    }

    fn read<T: serde::de::DeserializeOwned>(
        response: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<T> {
        let mut response = response.map_err(|e| SpentDayError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let to = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("another address")
                .to_owned();
            return Err(SpentDayError::Server {
                status,
                message: format!(
                    "the server redirects to {to}. Spent Day does not follow redirects, so enter the address of the sync server itself."
                ),
            });
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|e| SpentDayError::Network(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(SpentDayError::Server {
                status,
                message: body,
            });
        }
        serde_json::from_str(&body)
            .map_err(|e| SpentDayError::Network(format!("bad response from server: {e}")))
    }

    pub fn push(&self, batch: &Batch) -> Result<()> {
        let body = serde_json::to_string(&self.keys.seal(&batch.device.id, batch))
            .expect("envelope serializes");
        let response = self
            .agent
            .post(&self.batches_url())
            .header("Authorization", &self.bearer())
            .header("Content-Type", "application/json")
            .send(body);
        Self::read::<PushResponse>(response).map(|_| ())
    }

    pub fn pull(&self, after: i64) -> Result<Page> {
        let url = format!("{}?after={after}&limit={PULL_PAGE}", self.batches_url());
        let response = self
            .agent
            .get(&url)
            .header("Authorization", &self.bearer())
            .call();
        Self::read(response)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{SpentDay, SyncConfig};

    #[test]
    fn code_round_trips_and_tolerates_typing() {
        let secret = [7u8; 32];
        let code = encode_code(&secret);
        assert_eq!(
            code,
            "0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3M-QC3F"
        );
        assert_eq!(decode_code(&code).unwrap(), secret);
        let sloppy = code
            .to_lowercase()
            .replace('-', " ")
            .replace('1', "l")
            .replace('0', "o");
        assert_eq!(decode_code(&sloppy).unwrap(), secret);
    }

    #[test]
    fn code_rejects_typos_and_bad_lengths() {
        let code = encode_code(&[7u8; 32]);
        let typo = code.replacen("W3G", "W3H", 1);
        assert!(matches!(
            decode_code(&typo),
            Err(SpentDayError::InvalidSyncCode)
        ));
        assert!(matches!(
            decode_code(&code[..code.len() - 1]),
            Err(SpentDayError::InvalidSyncCode)
        ));
        assert!(matches!(
            decode_code("hello"),
            Err(SpentDayError::InvalidSyncCode)
        ));
    }

    #[test]
    fn batch_from_another_key_fails_to_decrypt() {
        let a = Keys::derive(&encode_code(&[1u8; 32])).unwrap();
        let b = Keys::derive(&encode_code(&[2u8; 32])).unwrap();
        let batch = Batch {
            device: WireDevice {
                id: "d1".into(),
                name: "n".into(),
                platform: Platform::Linux,
            },
            spans: vec![],
        };
        let sealed = a.seal("d1", &batch);
        assert_eq!(a.open(&sealed).unwrap().device.id, "d1");
        assert!(matches!(b.open(&sealed), Err(SpentDayError::Decrypt)));
        let relabelled = Envelope {
            device_id: "d2".into(),
            ..sealed
        };
        assert!(matches!(a.open(&relabelled), Err(SpentDayError::Decrypt)));
    }

    fn start_server(dir: &tempfile::TempDir) -> String {
        let conn = spentday_server::open_db(&dir.path().join("server.db").to_string_lossy()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                spentday_server::serve(tokio::net::TcpListener::from_std(listener).unwrap(), conn)
                    .await
                    .unwrap();
            });
        });
        url
    }

    fn batch(device: &str) -> Batch {
        Batch {
            device: WireDevice {
                id: device.into(),
                name: "n".into(),
                platform: Platform::Linux,
            },
            spans: vec![],
        }
    }

    #[test]
    fn wrong_token_gets_401_and_wrong_key_gets_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let server = start_server(&dir);
        let owner = Keys::derive(&encode_code(&[1u8; 32])).unwrap();
        let other = Keys::derive(&encode_code(&[2u8; 32])).unwrap();
        let space_id = owner.space_id.clone();
        let owner_token = owner.auth_token.clone();
        Client::new(&server, owner).push(&batch("d1")).unwrap();

        let intruder = Client::new(
            &server,
            Keys {
                space_id: space_id.clone(),
                auth_token: other.auth_token.clone(),
                enc_key: other.enc_key,
            },
        );
        match intruder.push(&batch("d2")) {
            Err(SpentDayError::Server { status, message }) => assert_eq!(
                (status, message.as_str()),
                (401, "wrong token for this space")
            ),
            other => panic!("expected 401, got {:?}", other.err()),
        }
        assert!(matches!(
            intruder.pull(0),
            Err(SpentDayError::Server { status: 401, .. })
        ));

        let wrong_key = Client::new(
            &server,
            Keys {
                space_id,
                auth_token: owner_token,
                enc_key: other.enc_key,
            },
        );
        let page = wrong_key.pull(0).unwrap();
        assert_eq!(page.batches.len(), 1);
        assert!(matches!(
            wrong_key.keys().open(&page.batches[0].envelope),
            Err(SpentDayError::Decrypt)
        ));
    }

    fn post(server: &str, keys: &Keys, envelope: &Envelope) {
        let mut response = ureq::post(&format!("{server}/v1/spaces/{}/batches", keys.space_id))
            .header("Authorization", &format!("Bearer {}", keys.auth_token))
            .header("Content-Type", "application/json")
            .send(serde_json::to_string(envelope).unwrap())
            .unwrap();
        assert_eq!(
            response.status(),
            200,
            "{:?}",
            response.body_mut().read_to_string()
        );
    }

    fn device(dir: &tempfile::TempDir, file: &str, server: &str, code: &str) -> Arc<SpentDay> {
        let m = SpentDay::open(
            dir.path().join(file).to_string_lossy().into_owned(),
            file.into(),
            Platform::Linux,
        )
        .unwrap();
        m.set_sync_config(Some(SyncConfig {
            server_url: server.into(),
            sync_code: code.into(),
        }))
        .unwrap();
        m
    }

    fn span(id: &str, end_ms: i64) -> serde_json::Value {
        serde_json::json!({
            "device_id": "", "id": id, "start_ms": 0, "end_ms": end_ms,
            "app_id": "org.gnome.Terminal", "app_name": "Terminal",
            "domain": null, "rev": 1,
        })
    }

    #[test]
    fn batches_that_do_not_open_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let server = start_server(&dir);
        let code = encode_code(&[3u8; 32]);
        let keys = Keys::derive(&code).unwrap();
        let other = Keys::derive(&encode_code(&[4u8; 32])).unwrap();
        let a = device(&dir, "a.db", &server, &code);
        a.record_interval(
            0,
            1_000,
            crate::Activity {
                app_id: "org.gnome.Terminal".into(),
                app_name: "Terminal".into(),
                web: None,
            },
        )
        .unwrap();

        let junk = Envelope {
            device_id: "junk".into(),
            nonce: B64.encode([0u8; 24]),
            ciphertext: B64.encode(b"not a sealed batch"),
        };
        post(&server, &keys, &junk);
        post(
            &server,
            &keys,
            &other.seal("foreign", &serde_json::json!({})),
        );
        post(&server, &keys, &keys.seal("not-json", &"[1, 2"));
        assert_eq!(a.sync_now().unwrap().pushed_spans, 1);
        let wrong_shape = serde_json::json!({ "device": { "id": "shape" }, "spans": [] });
        post(&server, &keys, &keys.seal("shape", &wrong_shape));

        let b = device(&dir, "b.db", &server, &code);
        let report = b.sync_now().unwrap();
        assert_eq!(
            (
                report.pushed_spans,
                report.pulled_spans,
                report.skipped_batches
            ),
            (0, 1, 4)
        );
        let again = b.sync_now().unwrap();
        assert_eq!((again.pulled_spans, again.skipped_batches), (0, 0));
        let ids: Vec<_> = b.devices().unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(ids, vec!["b.db", "a.db"]);
    }

    #[test]
    fn an_unknown_platform_still_merges() {
        let dir = tempfile::tempdir().unwrap();
        let server = start_server(&dir);
        let code = encode_code(&[5u8; 32]);
        let keys = Keys::derive(&code).unwrap();
        let batch = serde_json::json!({
            "device": { "id": "cros", "name": "Chromebook", "platform": "chromeos" },
            "spans": [span("s1", 7_000)],
        });
        post(&server, &keys, &keys.seal("cros", &batch));

        let b = device(&dir, "b.db", &server, &code);
        let report = b.sync_now().unwrap();
        assert_eq!((report.pulled_spans, report.skipped_batches), (1, 0));
        let devices: Vec<_> = b
            .devices()
            .unwrap()
            .into_iter()
            .map(|d| (d.name, d.platform, d.last_seen_ms))
            .collect();
        assert_eq!(
            devices,
            vec![
                ("b.db".to_owned(), Platform::Linux, 0),
                ("Chromebook".to_owned(), Platform::Linux, 7_000),
            ]
        );
    }

    #[test]
    fn plain_http_is_only_for_this_device_and_private_networks() {
        for ok in [
            "https://sync.example.com",
            "https://203.0.113.9:8443",
            "http://127.0.0.1:8787",
            "http://127.5.0.1",
            "http://localhost:8787",
            "http://LOCALHOST",
            "http://[::1]:8787",
            "http://10.0.2.2:8787",
            "http://172.16.0.1",
            "http://172.31.255.255",
            "http://192.168.1.20:8787",
            "http://[fd12::1]",
            "http://[fc00::1]",
            "https://nas.local:8787",
        ] {
            assert!(check_server_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://sync.example.com",
            "http://8.8.8.8",
            "http://172.32.0.1",
            "http://[2001:db8::1]",
            "http://localhost.example.com",
            "http://nas.local.example.com",
            // A .local name over plain http would go to whatever mDNS answers.
            "http://nas.local:8787",
            "http://nas.local.:8787",
            "ftp://sync.example.com",
            "sync.example.com",
            "http://",
        ] {
            assert!(
                matches!(check_server_url(bad), Err(SpentDayError::InvalidArgument(_))),
                "{bad}"
            );
        }
        assert_eq!(
            check_server_url("http://10.0.0.1.:8787").unwrap(),
            "http://10.0.0.1:8787"
        );
        // Plain http to localhost never asks a resolver.
        for localhost in ["http://localhost:8787", "http://LOCALHOST.:8787/"] {
            assert_eq!(check_server_url(localhost).unwrap(), "http://127.0.0.1:8787");
        }
        assert_eq!(
            check_server_url("https://localhost:8787").unwrap(),
            "https://localhost:8787"
        );
        assert_eq!(
            check_server_url("HTTPS://Sync.Example.com/spentday/").unwrap(),
            "https://sync.example.com/spentday"
        );
        let dir = tempfile::tempdir().unwrap();
        let m = SpentDay::open(
            dir.path().join("m.db").to_string_lossy().into_owned(),
            "box".into(),
            Platform::Linux,
        )
        .unwrap();
        let config = |server_url: &str| {
            Some(SyncConfig {
                server_url: server_url.into(),
                sync_code: m.new_sync_code(),
            })
        };
        match m.set_sync_config(config("http://sync.example.com")) {
            Err(SpentDayError::InvalidArgument(why)) => assert_eq!(
                why,
                "Use https:// for sync.example.com. Plain http:// only works for this device \
                 or a private network address."
            ),
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
        assert_eq!(m.sync_config().unwrap(), None);
    }

    /// The plaintext of every batch a device pushes, as the server would hand
    /// it to another device holding the code.
    fn pushed_plaintexts(server: &str, code: &str) -> Vec<String> {
        let keys = Keys::derive(code).unwrap();
        let page = Client::new(server, Keys::derive(code).unwrap()).pull(0).unwrap();
        page.batches
            .iter()
            .map(|item| {
                let envelope = &item.envelope;
                let nonce: [u8; 24] = B64.decode(&envelope.nonce).unwrap().try_into().unwrap();
                let plaintext = keys
                    .cipher()
                    .decrypt(
                        &XNonce::from(nonce),
                        Payload {
                            msg: &B64.decode(&envelope.ciphertext).unwrap(),
                            aad: &keys.aad(&envelope.device_id),
                        },
                    )
                    .unwrap();
                String::from_utf8(plaintext).unwrap()
            })
            .collect()
    }

    #[test]
    fn sync_batches_carry_domains_and_app_names_only() {
        let dir = tempfile::tempdir().unwrap();
        let server = start_server(&dir);
        let code = encode_code(&[6u8; 32]);
        let a = device(&dir, "a.db", &server, &code);
        let url = "https://www.example.com/secret-path?q=secret-query#secret-fragment";
        let page = || crate::WebPage { url: url.into() };
        let chrome = |app_id: &str, web| crate::Activity {
            app_id: app_id.into(),
            app_name: "Chrome".into(),
            web,
        };
        a.heartbeat(0, Some(chrome("com.google.Chrome", Some(page()))))
            .unwrap();
        a.heartbeat(5_000, Some(chrome("com.google.Chrome", Some(page()))))
            .unwrap();
        a.flush(5_000).unwrap();
        a.web_hint(10_000, "com.android.chrome".into(), page()).unwrap();
        a.record_interval(10_000, 20_000, chrome("com.android.chrome", None))
            .unwrap();
        assert_eq!(a.sync_now().unwrap().pushed_spans, 2);

        let plaintexts = pushed_plaintexts(&server, &code);
        assert_eq!(plaintexts.len(), 1);
        for text in &plaintexts {
            assert!(text.contains("\"domain\":\"example.com\""), "{text}");
            for secret in ["secret-path", "secret-query", "secret-fragment", "title", "url"] {
                assert!(!text.contains(secret), "{secret} in {text}");
            }
            let batch: serde_json::Value = serde_json::from_str(text).unwrap();
            for span in batch["spans"].as_array().unwrap() {
                let mut keys: Vec<_> = span.as_object().unwrap().keys().cloned().collect();
                keys.sort();
                assert_eq!(
                    keys,
                    [
                        "app_id", "app_name", "device_id", "domain", "end_ms", "id", "rev",
                        "start_ms"
                    ]
                );
            }
        }
    }

    /// Answers every request with a 302 to `to`, and counts the requests.
    fn redirector(to: String) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut head = [0u8; 4096];
                let _ = stream.read(&mut head);
                let _ = write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        });
        (url, hits)
    }

    #[test]
    fn redirects_are_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        // Where a hostile server would send the client: plain http to an
        // address the https rule refuses.
        let (target, target_hits) = redirector("http://0.0.0.0:1/".into());
        let (server, server_hits) = redirector(format!("{target}/v1/spaces/x/batches"));
        let code = encode_code(&[8u8; 32]);
        let client = Client::new(&server, Keys::derive(&code).unwrap());
        for result in [client.pull(0).map(|_| ()), client.push(&batch("d1"))] {
            match result {
                Err(SpentDayError::Server { status: 302, message }) => {
                    assert!(message.contains("does not follow redirects"), "{message}")
                }
                other => panic!("expected the 302 as an error, got {:?}", other.err()),
            }
        }
        let m = device(&dir, "m.db", &server, &code);
        assert!(matches!(
            m.sync_now(),
            Err(SpentDayError::Server { status: 302, .. })
        ));
        assert!(server_hits.load(std::sync::atomic::Ordering::SeqCst) >= 3);
        assert_eq!(target_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
