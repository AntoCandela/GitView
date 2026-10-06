//! Accounts conservatively for decoded values and preserves explicit collection truncation on eviction.
use super::*;
use crate::github::{model::Completeness, transport::Connection};
use serde_json::Value;

pub(crate) struct CachedPage {
    pub(super) observation_id: Uuid,
    pub body: Value,
    pub observed_at: u64,
    pub items: usize,
    pub completeness: Completeness,
    pub confirmed_negative: bool,
}
pub(super) fn decode(
    read: &GhRead,
    response: ApiResponse,
    observed_at: u64,
) -> Result<CachedPage, Failure> {
    let body = response.body;
    let (items, more) = match read {
        GhRead::ListPulls { .. } | GhRead::ReadPullFiles { .. } => {
            let items = body
                .as_array()
                .ok_or_else(|| PrCode::InvalidOutput.failure())?
                .len();
            (items, response.has_next)
        }
        GhRead::ReadCommit { .. } => {
            let items = body
                .get("files")
                .and_then(Value::as_array)
                .ok_or_else(|| PrCode::InvalidOutput.failure())?
                .len();
            (items, response.has_next)
        }
        GhRead::ReadConnection { connection, .. } => {
            let name = match connection {
                Connection::Commits => "commits",
                Connection::Timeline => "timelineItems",
                Connection::Threads => "reviewThreads",
                Connection::Reviewers => "reviewRequests",
                Connection::Labels => "labels",
                Connection::ThreadComments { .. } => "comments",
            };
            let parent = if matches!(connection, Connection::ThreadComments { .. }) {
                body.pointer("/data/node")
            } else {
                body.pointer("/data/repository/pullRequest")
            };
            let connection = parent
                .and_then(|parent| parent.get(name))
                .ok_or_else(|| PrCode::InvalidOutput.failure())?;
            let items = connection
                .get("nodes")
                .and_then(Value::as_array)
                .ok_or_else(|| PrCode::InvalidOutput.failure())?
                .len();
            let more = connection
                .pointer("/pageInfo/hasNextPage")
                .and_then(Value::as_bool)
                .ok_or_else(|| PrCode::InvalidOutput.failure())?;
            (items, more)
        }
        GhRead::ReadOverview { .. } | GhRead::ReadPull { .. } | GhRead::ReadRepository { .. } if body.is_object() => (1, false),
        _ => return Err(PrCode::InvalidOutput.failure()),
    };
    if items > 100 {
        return Err(PrCode::ResourceLimit.failure());
    }
    let confirmed_negative = matches!(read, GhRead::ListPulls { page: 1, .. }) && items == 0 && !more;
    let completeness = if more {
        Completeness::More
    } else {
        Completeness::Complete
    };
    Ok(CachedPage {
        observation_id: Uuid::new_v4(),
        body,
        observed_at,
        items,
        completeness,
        confirmed_negative,
    })
}

/// Includes container capacities and conservative per-map-node allocation overhead, not JSON bytes.
fn decoded_bytes(value: &Value) -> usize {
    let own = std::mem::size_of::<Value>();
    own.saturating_add(match value {
        Value::String(value) => value.capacity(),
        Value::Array(values) => values
            .capacity()
            .saturating_mul(own)
            .saturating_add(values.iter().map(decoded_bytes).sum::<usize>()),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                256usize
                    .saturating_add(key.capacity())
                    .saturating_add(decoded_bytes(value))
            })
            .sum(),
        _ => 0,
    })
}
pub(super) fn retain(state: &mut State, key: &Key, page: CachedPage) -> Result<(), Failure> {
    let bytes = decoded_bytes(&page.body).saturating_add(std::mem::size_of::<CachedPage>());
    if bytes > MAX_BYTES || page.items > MAX_ITEMS {
        return Err(PrCode::ResourceLimit.failure());
    }
    if let Some(entry) = state.entries.get_mut(key) {
        state.bytes = state.bytes.saturating_sub(entry.bytes);
        entry.bytes = 0;
        entry.page = None;
    }
    let group = key.collection();
    loop {
        let (pages, items) = state
            .entries
            .iter()
            .filter(|(other, _)| other.collection() == group)
            .fold((0, 0), |(pages, items), (_, entry)| match &entry.page {
                Some(page) => (pages + 1, items + page.items),
                None => (pages, items),
            });
        let group_full = pages >= MAX_PAGES || items.saturating_add(page.items) > MAX_ITEMS;
        if !group_full && state.bytes.saturating_add(bytes) <= MAX_BYTES {
            break;
        }
        let victim = state
            .entries
            .iter()
            .filter(|(other, entry)| {
                *other != key
                    && entry.page.is_some()
                    && (!group_full || other.collection() == group)
            })
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone())
            .ok_or_else(|| PrCode::ResourceLimit.failure())?;
        if let Some(group) = state.collections.get_mut(&victim.collection()) {
            group.limited = true;
        }
        if let Some(entry) = state.entries.get_mut(&victim) {
            state.bytes = state.bytes.saturating_sub(entry.bytes);
            entry.bytes = 0;
            entry.page = None;
            entry.authorized = false;
            entry.failure = Some(PrCode::ResourceLimit.failure());
        }
    }
    state.used = state.used.saturating_add(1);
    let used = state.used;
    let entry = state
        .entries
        .get_mut(key)
        .ok_or_else(|| PrCode::StaleContext.failure())?;
    entry.page = Some(Arc::new(page));
    entry.bytes = bytes;
    entry.used = used;
    entry.observed = tokio::time::Instant::now();
    entry.authorized = true;
    entry.failure = None;
    state.bytes += bytes;
    Ok(())
}
