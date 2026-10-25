//! Append-only store of encrypted Spent Day batches. It never sees plaintext: each
//! space is a namespace derived from a sync secret, claimed by the first writer.

use std::sync::{Arc, Mutex};

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_LIMIT: i64 = 200;
const MAX_LIMIT: i64 = 200;
/// A page stops before it passes this many bytes of batches, but always holds
/// at least one, so a client can read any page with a fixed limit near
/// `PAGE_BYTES + MAX_BODY_BYTES`.
pub const PAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
struct Db(Arc<Mutex<Connection>>);

pub fn open_db(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS spaces (
            space_id TEXT PRIMARY KEY,
            token_sha256 BLOB NOT NULL
         ) STRICT;
         CREATE TABLE IF NOT EXISTS batches (
            cursor INTEGER PRIMARY KEY AUTOINCREMENT,
            space_id TEXT NOT NULL,
            device_id TEXT NOT NULL,
            nonce TEXT NOT NULL,
            ciphertext TEXT NOT NULL
         ) STRICT;
         CREATE INDEX IF NOT EXISTS batches_space_cursor ON batches (space_id, cursor);",
    )?;
    Ok(conn)
}

pub fn router(conn: Connection) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/spaces/{space_id}/batches", post(push).get(pull))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(Db(Arc::new(Mutex::new(conn))))
}

/// Serves until the listener fails. Used by `main` and by in-process tests.
pub async fn serve(listener: tokio::net::TcpListener, conn: Connection) -> std::io::Result<()> {
    axum::serve(listener, router(conn)).await
}

struct ApiError(StatusCode, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn token_hash(headers: &HeaderMap) -> Result<Vec<u8>, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing bearer token"))?;
    Ok(Sha256::digest(token.as_bytes()).to_vec())
}

/// Checks the token against the space's stored hash. A space nobody has written
/// to yet is claimed by the first writer (`claim`), and reads from it see nothing.
fn authorize(
    conn: &Connection,
    space_id: &str,
    hash: &[u8],
    claim: bool,
) -> Result<bool, ApiError> {
    let stored: Option<Vec<u8>> = conn
        .query_row(
            "SELECT token_sha256 FROM spaces WHERE space_id = ?1",
            [space_id],
            |r| r.get(0),
        )
        .optional()?;
    match stored {
        Some(stored) if stored == hash => Ok(true),
        Some(_) => Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "wrong token for this space",
        )),
        None if claim => {
            conn.execute(
                "INSERT INTO spaces (space_id, token_sha256) VALUES (?1, ?2)",
                params![space_id, hash],
            )?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[derive(Deserialize)]
struct PushBody {
    device_id: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize)]
struct PushReply {
    cursor: i64,
}

async fn push(
    State(db): State<Db>,
    Path(space_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<PushBody>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<PushReply>, ApiError> {
    if !valid_id(&space_id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad space id"));
    }
    let hash = token_hash(&headers)?;
    let Json(body) = body.map_err(|e| match e.status() {
        StatusCode::PAYLOAD_TOO_LARGE => ApiError(StatusCode::PAYLOAD_TOO_LARGE, "body over 8 MiB"),
        _ => ApiError(
            StatusCode::BAD_REQUEST,
            "expected {device_id, nonce, ciphertext}",
        ),
    })?;
    if !valid_id(&body.device_id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad device id"));
    }
    if B64.decode(&body.nonce).map(|n| n.len()) != Ok(24) || B64.decode(&body.ciphertext).is_err() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "nonce and ciphertext must be base64, nonce 24 bytes",
        ));
    }
    tokio::task::spawn_blocking(move || {
        let mut conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.transaction()?;
        authorize(&tx, &space_id, &hash, true)?;
        tx.execute(
            "INSERT INTO batches (space_id, device_id, nonce, ciphertext) VALUES (?1, ?2, ?3, ?4)",
            params![space_id, body.device_id, body.nonce, body.ciphertext],
        )?;
        let cursor = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Json(PushReply { cursor }))
    })
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker panicked"))?
}

#[derive(Deserialize)]
struct PullParams {
    after: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct PulledBatch {
    cursor: i64,
    device_id: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize)]
struct PullReply {
    batches: Vec<PulledBatch>,
    more: bool,
    /// The newest cursor in the space, `0` when it is empty. Clients whose
    /// stored cursor is past it read the space again from the start.
    head: i64,
}

async fn pull(
    State(db): State<Db>,
    Path(space_id): Path<String>,
    Query(params): Query<PullParams>,
    headers: HeaderMap,
) -> Result<Json<PullReply>, ApiError> {
    if !valid_id(&space_id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad space id"));
    }
    let hash = token_hash(&headers)?;
    let after = params.after.unwrap_or(0);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    tokio::task::spawn_blocking(move || {
        let conn = db.0.lock().unwrap_or_else(|p| p.into_inner());
        if !authorize(&conn, &space_id, &hash, false)? {
            return Ok(Json(PullReply {
                batches: vec![],
                more: false,
                head: 0,
            }));
        }
        let head: i64 = conn.query_row(
            "SELECT COALESCE(MAX(cursor), 0) FROM batches WHERE space_id = ?1",
            [&space_id],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare_cached(
            "SELECT cursor, device_id, nonce, ciphertext FROM batches
             WHERE space_id = ?1 AND cursor > ?2 ORDER BY cursor LIMIT ?3",
        )?;
        let mut rows = stmt.query_map(params![space_id, after, limit + 1], |r| {
            Ok(PulledBatch {
                cursor: r.get(0)?,
                device_id: r.get(1)?,
                nonce: r.get(2)?,
                ciphertext: r.get(3)?,
            })
        })?;
        let mut batches = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        for row in &mut rows {
            let batch = row?;
            bytes += batch.device_id.len() + batch.nonce.len() + batch.ciphertext.len();
            if batches.len() as i64 == limit || (!batches.is_empty() && bytes > PAGE_BYTES) {
                more = true;
                break;
            }
            batches.push(batch);
        }
        Ok(Json(PullReply {
            batches,
            more,
            head,
        }))
    })
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker panicked"))?
}
