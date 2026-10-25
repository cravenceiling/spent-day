//! Turns heartbeats and intervals into spans.

use sha2::{Digest, Sha256};

use crate::store::{Span, Store, meta};
use crate::web;
use crate::{Activity, SpentDayError, Millis};

/// Heartbeats further apart than this close the open span.
pub const MAX_GAP_MS: Millis = 30_000;

type Result<T> = std::result::Result<T, SpentDayError>;

/// The parts of a span that decide whether a heartbeat extends it.
fn key(span: &Span) -> (&str, Option<&str>) {
    (&span.app_id, span.domain.as_deref())
}

fn new_span(
    id: String,
    start_ms: Millis,
    end_ms: Millis,
    app: &App,
    domain: Option<String>,
) -> Span {
    Span {
        device_id: String::new(),
        id,
        start_ms,
        end_ms,
        app_id: app.id.clone(),
        app_name: app.name.clone(),
        domain,
        rev: 0,
    }
}

/// What the core keeps of an `Activity`: the app, cut to the stored length,
/// and the domain of its page when the collector read one. The URL itself
/// goes no further.
struct App {
    id: String,
    name: String,
    /// `None` when the collector read no page. `Some(None)` for a page that
    /// has no domain (`chrome://`, `file://`).
    page: Option<Option<String>>,
}

fn app(activity: Activity) -> App {
    App {
        id: web::clip(activity.app_id, web::MAX_TEXT_BYTES),
        name: web::clip(activity.app_name, web::MAX_TEXT_BYTES),
        page: activity.web.map(|page| web::normalize_domain(&page.url)),
    }
}

/// The open heartbeat span. Its id lives in `meta` so a restarted process (or the
/// next CLI invocation) keeps extending it.
fn open_span(store: &Store) -> Result<Option<Span>> {
    match store.meta(meta::OPEN_SPAN)? {
        Some(id) => store.span(&id),
        None => Ok(None),
    }
}

/// Extends `open` to `at_ms` when the heartbeat continues it in time.
fn reaches(open: &Span, at_ms: Millis) -> bool {
    at_ms >= open.end_ms && at_ms - open.end_ms <= MAX_GAP_MS
}

pub fn heartbeat(store: &mut Store, at_ms: Millis, activity: Option<Activity>) -> Result<()> {
    let Some(app) = activity.map(app) else {
        return close(store, at_ms);
    };
    let domain = match &app.page {
        Some(domain) => domain.clone(),
        None => web::domain_at(store, &app.id, at_ms)?,
    };
    let mut candidate = new_span(uuid::Uuid::new_v4().to_string(), at_ms, at_ms, &app, domain);

    let open = open_span(store)?;
    // The clock stepped back or a callback arrived late. Time before the
    // latest end this device recorded is already counted, even when the span
    // holding it is closed, so never start a span inside it.
    if let Some(floor) = store.own_spans_end()?.filter(|&end| at_ms < end) {
        if open.is_some_and(|open| key(&open) == key(&candidate)) {
            return Ok(());
        }
        candidate.start_ms = floor;
        candidate.end_ms = floor;
    } else if let Some(mut open) = open
        && reaches(&open, at_ms)
    {
        open.end_ms = at_ms;
        if key(&open) == key(&candidate) {
            return store.put_own_span(open);
        }
        store.put_own_span(open)?;
    }
    let id = candidate.id.clone();
    store.put_own_span(candidate)?;
    store.set_meta(meta::OPEN_SPAN, Some(&id))
}

pub fn close(store: &mut Store, at_ms: Millis) -> Result<()> {
    away_since(store, Millis::MAX, at_ms)
}

/// Closes the open span as `close(now_ms)` would, then cuts every heartbeat
/// span back to the user's last input. Heartbeats kept arriving while the
/// user was idle, and an app, site or hint change in that time split them
/// into more spans than the open one.
pub fn away_since(store: &mut Store, last_input_ms: Millis, now_ms: Millis) -> Result<()> {
    let open = open_span(store)?.map(|mut open| {
        if reaches(&open, now_ms) {
            open.end_ms = now_ms;
        }
        open
    });
    store.close_heartbeat_spans(open, last_input_ms)
}

/// Span id for an interval piece: the same `(device, app_id, start)` always maps
/// to the same id, so re-ingesting a UsageStats window rewrites rows in place.
/// The first piece's id is also the interval's id.
fn piece_id(device_id: &str, app_id: &str, start_ms: Millis) -> String {
    let mut h = Sha256::new();
    h.update(device_id.as_bytes());
    h.update([0]);
    h.update(app_id.as_bytes());
    h.update([0]);
    h.update(start_ms.to_be_bytes());
    hex::encode(&h.finalize()[..16])
}

pub fn record_interval(
    store: &mut Store,
    start_ms: Millis,
    end_ms: Millis,
    activity: Activity,
) -> Result<()> {
    let app = app(activity);
    let interval_id = piece_id(&store.device_id, &app.id, start_ms);
    if end_ms <= start_ms {
        return store.put_interval(&interval_id, Vec::new());
    }
    let pieces = match &app.page {
        Some(domain) => vec![web::Piece {
            start_ms,
            end_ms,
            domain: domain.clone(),
        }],
        None => web::split_interval(store, &app.id, start_ms, end_ms)?,
    };
    let spans = pieces
        .into_iter()
        .map(|piece| {
            new_span(
                piece_id(&store.device_id, &app.id, piece.start_ms),
                piece.start_ms,
                piece.end_ms,
                &app,
                piece.domain,
            )
        })
        .collect();
    store.put_interval(&interval_id, spans)
}
