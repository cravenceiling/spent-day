//! Totals over a time range.

use std::collections::HashMap;

use crate::store::Span;
use crate::{
    AppUsage, DeviceInfo, DeviceUsage, DomainUsage, SpentDayError, Millis, Summary, SummaryQuery,
    apps,
};

#[derive(Default)]
struct AppTotal {
    name: String,
    name_seen_at: Millis,
    total_ms: Millis,
    domains: HashMap<String, Millis>,
    app_ids: HashMap<String, Millis>,
}

/// Keys by time, descending, ties by key.
fn ranked(map: HashMap<String, Millis>) -> Vec<(String, Millis)> {
    let mut out: Vec<_> = map.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

fn sorted_domains(map: HashMap<String, Millis>) -> Vec<DomainUsage> {
    ranked(map)
        .into_iter()
        .map(|(domain, total_ms)| DomainUsage { domain, total_ms })
        .collect()
}

/// A day in minutes is 1_440 buckets, so this only rejects queries that would
/// allocate far more than any chart can show.
const MAX_BUCKETS: i128 = 10_000;

/// Number of `bucket_ms` slots in `[start_ms, end_ms)`, `0` when bucketing is off.
fn bucket_count(query: &SummaryQuery) -> Result<usize, SpentDayError> {
    let SummaryQuery {
        start_ms,
        end_ms,
        bucket_ms,
        ..
    } = *query;
    if bucket_ms <= 0 || end_ms <= start_ms {
        return Ok(0);
    }
    let len = end_ms as i128 - start_ms as i128;
    let count = (len + bucket_ms as i128 - 1) / bucket_ms as i128;
    if count > MAX_BUCKETS {
        return Err(SpentDayError::InvalidArgument(format!(
            "bucket_ms {bucket_ms} splits the range into {count} buckets, more than {MAX_BUCKETS}"
        )));
    }
    Ok(count as usize)
}

pub fn summarize(
    query: &SummaryQuery,
    spans: &[Span],
    devices: &[DeviceInfo],
) -> Result<Summary, SpentDayError> {
    let bucket_count = bucket_count(query)?;
    let SummaryQuery {
        start_ms,
        end_ms,
        bucket_ms,
        ..
    } = *query;
    let mut buckets: Vec<Millis> = vec![0; bucket_count];
    let mut total_ms: Millis = 0;
    let mut by_device: HashMap<&str, Millis> = HashMap::new();
    let mut by_app: HashMap<String, AppTotal> = HashMap::new();
    let mut sites: HashMap<String, Millis> = HashMap::new();

    for span in spans {
        let from = span.start_ms.max(start_ms);
        let to = span.end_ms.min(end_ms);
        if to <= from {
            continue;
        }
        let ms = to.saturating_sub(from);
        total_ms = total_ms.saturating_add(ms);
        let device = by_device.entry(&span.device_id).or_default();
        *device = device.saturating_add(ms);

        let known = apps::lookup(&span.app_id);
        let app = by_app.entry(apps::app_key(&span.app_id)).or_default();
        app.total_ms = app.total_ms.saturating_add(ms);
        let id = app.app_ids.entry(span.app_id.clone()).or_default();
        *id = id.saturating_add(ms);
        if app.name.is_empty() || span.end_ms > app.name_seen_at {
            app.name = known.map_or_else(|| span.app_name.clone(), |a| a.name.to_owned());
            app.name_seen_at = span.end_ms;
        }
        if let Some(domain) = &span.domain {
            let site = sites.entry(domain.clone()).or_default();
            *site = site.saturating_add(ms);
        }
        if known.is_some_and(|a| a.browser) {
            let domain = span.domain.clone().unwrap_or_default();
            let in_app = app.domains.entry(domain).or_default();
            *in_app = in_app.saturating_add(ms);
        }

        if bucket_count > 0 {
            let (start, bucket, to) = (start_ms as i128, bucket_ms as i128, to as i128);
            let mut t = from as i128;
            while t < to {
                let slot = (t - start) / bucket;
                let slot_end = (start + (slot + 1) * bucket).min(to);
                let total = &mut buckets[slot as usize];
                *total = total.saturating_add((slot_end - t) as Millis);
                t = slot_end;
            }
        }
    }

    let mut by_device: Vec<DeviceUsage> = devices
        .iter()
        .filter_map(|d| {
            by_device.get(d.id.as_str()).map(|&total_ms| DeviceUsage {
                device: d.clone(),
                total_ms,
            })
        })
        .collect();
    by_device.sort_by(|a, b| {
        b.total_ms
            .cmp(&a.total_ms)
            .then_with(|| a.device.name.cmp(&b.device.name))
    });

    let mut apps: Vec<AppUsage> = by_app
        .into_iter()
        .map(|(app_key, t)| {
            let is_browser = apps::is_browser(&app_key);
            AppUsage {
                app_key,
                app_name: t.name,
                is_browser,
                total_ms: t.total_ms,
                domains: sorted_domains(t.domains),
                app_ids: ranked(t.app_ids).into_iter().map(|(id, _)| id).collect(),
            }
        })
        .collect();
    apps.sort_by(|a, b| {
        b.total_ms
            .cmp(&a.total_ms)
            .then_with(|| a.app_key.cmp(&b.app_key))
    });

    Ok(Summary {
        start_ms,
        end_ms,
        total_ms,
        by_device,
        apps,
        sites: sorted_domains(sites),
        buckets,
    })
}
