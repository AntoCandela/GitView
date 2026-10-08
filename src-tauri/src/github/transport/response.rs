//! Validates bounded provider framing and structured failures without inspecting human stderr.
use serde_json::Value;
use super::{Failure, PrCode, MAX_JSON_COLLECTION, MAX_JSON_DEPTH, MAX_JSON_NODES};

/// Only allowlisted numeric rate facts leave the framing parser. Times are Unix milliseconds.
#[derive(Default, Debug)]
pub(crate) struct RateHeaders { pub remaining: Option<u64>, pub retry_at: Option<u64> }
pub(crate) struct ApiResponse { pub status: u16, pub rate: RateHeaders, pub body: Value, pub has_next: bool }

pub(super) fn classify_version(bytes: &[u8], success: bool) -> Result<(), PrCode> {
    let text = std::str::from_utf8(bytes).map_err(|_| PrCode::GhUnsupported)?;
    let version = text.lines().next().and_then(|line| line.strip_prefix("gh version "))
        .and_then(|line| line.split_whitespace().next()).ok_or(PrCode::GhUnsupported)?;
    let parts: Vec<_> = version.split('.').collect();
    if !success || parts.len() != 3 { return Err(PrCode::GhUnsupported); }
    let mut values = Vec::new();
    for part in parts { values.push(part.parse::<u32>().map_err(|_| PrCode::GhUnsupported)?); }
    if values[0] < 2 || (values[0] == 2 && values[1] < 81) { return Err(PrCode::GhUnsupported); }
    Ok(())
}

pub(super) fn classify_auth(bytes: &[u8]) -> Result<(), PrCode> {
    let body = json(bytes)?;
    let hosts = body.get("hosts").and_then(Value::as_object).ok_or(PrCode::GhUnsupported)?;
    let Some(entries) = hosts.get("github.com") else { return Err(PrCode::AuthRequired); };
    let entries = entries.as_array().ok_or(PrCode::GhUnsupported)?;
    let mut active = None;
    for entry in entries {
        if entry.get("host").and_then(Value::as_str) != Some("github.com") { return Err(PrCode::GhUnsupported); }
        let is_active = entry.get("active").and_then(Value::as_bool).ok_or(PrCode::GhUnsupported)?;
        if is_active && active.replace(entry).is_some() { return Err(PrCode::GhUnsupported); }
    }
    let entry = active.ok_or(PrCode::AuthRequired)?;
    match entry.get("state").and_then(Value::as_str) {
        Some("success") if entry.get("error").is_none() => Ok(()),
        Some("error" | "timeout") => Err(PrCode::AuthUnavailable),
        _ => Err(PrCode::GhUnsupported),
    }
}

#[cfg(test)]
pub(super) fn parse_api(bytes: &[u8], success: bool, now_seconds: u64) -> Result<ApiResponse, Failure> {
    parse_read_api(bytes, success, now_seconds, None)
}

pub(super) fn parse_read_api(bytes: &[u8], success: bool, now_seconds: u64, read: Option<&super::GhRead>) -> Result<ApiResponse, Failure> {
    if bytes.is_empty() && !success { return Err(PrCode::Network.failure()); }
    let text = std::str::from_utf8(bytes).map_err(|_| PrCode::InvalidOutput.failure())?;
    let (header, body) = text.split_once("\r\n\r\n").or_else(|| text.split_once("\n\r\n")).or_else(|| text.split_once("\n\n"))
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    if header.len() > 32 * 1024 { return Err(PrCode::ResourceLimit.failure()); }
    let mut lines = header.lines();
    let status_line = lines.next().ok_or_else(|| PrCode::InvalidOutput.failure())?;
    if status_line.bytes().any(|b| b < 32 || b == 127) { return Err(PrCode::InvalidOutput.failure()); }
    let mut status_fields = status_line.splitn(3, ' ');
    if !matches!(status_fields.next(), Some("HTTP/1.0" | "HTTP/1.1" | "HTTP/2.0" | "HTTP/2" | "HTTP/3.0" | "HTTP/3")) { return Err(PrCode::InvalidOutput.failure()); }
    let status_text = status_fields.next().ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let status = status_text.parse::<u16>().ok().filter(|s| status_text.len() == 3 && (200..600).contains(s)).ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let mut headers = std::collections::HashMap::new();
    for (index, line) in lines.enumerate() {
        if index >= 128 { return Err(PrCode::ResourceLimit.failure()); }
        let (name, value) = line.split_once(':').ok_or_else(|| PrCode::InvalidOutput.failure())?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) || value.bytes().any(|b| (b < 32 && b != b'\t') || b == 127) { return Err(PrCode::InvalidOutput.failure()); }
        let name = name.to_ascii_lowercase();
        if matches!(name.as_str(), "retry-after" | "x-ratelimit-reset" | "x-ratelimit-remaining" | "content-type" | "link") && headers.insert(name, value.trim()).is_some() { return Err(PrCode::InvalidOutput.failure()); }
    }
    let number = |key: &str| -> Result<Option<u64>, Failure> {
        headers.get(key).map(|value| value.parse::<u64>().map_err(|_| PrCode::InvalidOutput.failure())).transpose()
    };
    let remaining = number("x-ratelimit-remaining")?;
    let reset = number("x-ratelimit-reset")?;
    let retry = headers.get("retry-after").map(|value| {
        value.parse::<u64>().ok().and_then(|seconds| now_seconds.checked_add(seconds)).or_else(|| http_date(value)).ok_or_else(|| PrCode::InvalidOutput.failure())
    }).transpose()?;
    let retry_at = retry.into_iter().chain(reset.filter(|_| remaining == Some(0))).max()
        .map(|seconds| seconds.checked_mul(1000).filter(|v| *v <= i64::MAX as u64).ok_or_else(|| PrCode::InvalidOutput.failure())).transpose()?;
    let rate = RateHeaders { remaining, retry_at };
    let failure = match status {
        401 => Some(PrCode::AuthRequired),
        403 if rate.remaining == Some(0) || retry.is_some() => Some(PrCode::RateLimited),
        403 => Some(PrCode::AccessDenied),
        404 => Some(PrCode::RepositoryUnavailable),
        429 => Some(PrCode::RateLimited),
        501 => Some(PrCode::GhUnsupported),
        500..=599 => Some(PrCode::Network),
        200..=299 => None,
        _ => Some(PrCode::InvalidOutput),
    };
    if let Some(code) = failure { let mut failure = code.failure(); if code == PrCode::RateLimited { failure.retry_at = rate.retry_at; } return Err(failure); }
    if let Some(content_type) = headers.get("content-type") {
        let mime = content_type.split(';').next().unwrap_or_default().trim();
        if mime != "application/json" && mime != "application/vnd.github+json" { return Err(PrCode::InvalidOutput.failure()); }
    }
    let mut body = json(body.as_bytes()).map_err(PrCode::failure)?;
    let projected = super::partial::project(&mut body, read);
    if let Some(errors) = body.get("errors") {
        let errors = errors.as_array().filter(|v| !v.is_empty()).ok_or_else(|| PrCode::InvalidOutput.failure())?;
        // Authentication/access loss must invalidate private authority regardless of error order.
        let code = errors.iter().flat_map(|error| [error.get("type"), error.pointer("/extensions/type"), error.pointer("/extensions/code")]).flatten().map(|kind| {
            let kind = kind.as_str();
            match kind { Some("UNAUTHORIZED" | "UNAUTHENTICATED") => (5,PrCode::AuthRequired),
                Some("FORBIDDEN") => (4,PrCode::AccessDenied), Some("NOT_FOUND") => (3,PrCode::RepositoryUnavailable),
                Some("RATE_LIMITED") => (1,PrCode::RateLimited), _ => (2,PrCode::InvalidOutput) }
        }).max_by_key(|(priority,_)| *priority).map(|(_,code)|code).unwrap_or(PrCode::InvalidOutput);
        let mut failure = code.failure(); if code == PrCode::RateLimited { failure.retry_at = rate.retry_at; } return Err(failure);
    }
    if !success && !projected { return Err(PrCode::InvalidOutput.failure()); }
    let has_next = headers.get("link").map(|value| pagination(value)).transpose().map_err(PrCode::failure)?.unwrap_or(false);
    Ok(ApiResponse { status, rate, body, has_next })
}

/// Links supply exhaustion evidence only. Callers construct their next fixed page request;
/// no provider URL is ever followed as request authority.
fn pagination(mut remaining: &str) -> Result<bool, PrCode> {
    if remaining.is_empty() || remaining.len() > 16 * 1024 { return Err(PrCode::InvalidOutput); }
    let mut relations = std::collections::HashSet::new();
    loop {
        let (destination, tail) = remaining.trim_start().strip_prefix('<').and_then(|s| s.split_once('>')).ok_or(PrCode::InvalidOutput)?;
        let url = url::Url::parse(destination).map_err(|_| PrCode::InvalidOutput)?;
        if url.scheme() != "https" || url.host_str() != Some("api.github.com") || !url.username().is_empty()
            || url.password().is_some() || url.fragment().is_some() || url.port().is_some()
            || !url.path().starts_with("/repos/") { return Err(PrCode::InvalidOutput); }
        let (attributes, rest) = tail.split_once(',').map_or((tail, None), |(a, b)| (a, Some(b)));
        let relation = attributes.trim().strip_prefix("; rel=\"").and_then(|s| s.strip_suffix('"')).ok_or(PrCode::InvalidOutput)?;
        if !matches!(relation, "next" | "prev" | "first" | "last") || !relations.insert(relation.to_owned()) { return Err(PrCode::InvalidOutput); }
        match rest { Some(rest) if !rest.trim().is_empty() => remaining = rest, Some(_) => return Err(PrCode::InvalidOutput), None => break }
    }
    Ok(relations.contains("next"))
}

fn json(bytes: &[u8]) -> Result<Value, PrCode> {
    std::str::from_utf8(bytes).map_err(|_| PrCode::InvalidOutput)?;
    let body: Value = serde_json::from_slice(bytes).map_err(|_| PrCode::InvalidOutput)?;
    fn check(value: &Value, depth: usize, left: &mut usize) -> Result<(), PrCode> {
        if depth > MAX_JSON_DEPTH || *left == 0 { return Err(PrCode::ResourceLimit); } *left -= 1;
        match value {
            Value::Array(items) => { if items.len() > MAX_JSON_COLLECTION { return Err(PrCode::ResourceLimit); } for item in items { check(item, depth + 1, left)?; } }
            Value::Object(items) => { if items.len() > MAX_JSON_COLLECTION { return Err(PrCode::ResourceLimit); } for item in items.values() { check(item, depth + 1, left)?; } }
            _ => {}
        }
        Ok(())
    }
    let mut remaining = MAX_JSON_NODES;
    check(&body, 0, &mut remaining)?;
    Ok(body)
}

fn http_date(value: &str) -> Option<u64> {
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 6 || !["Mon,", "Tue,", "Wed,", "Thu,", "Fri,", "Sat,", "Sun,"].contains(&fields[0]) || fields[5] != "GMT" { return None; }
    let day = fields[1].parse::<i64>().ok()?;
    let month = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"].iter().position(|m| *m == fields[2])? as i64 + 1;
    let year = fields[3].parse::<i64>().ok().filter(|y| (1970..=9999).contains(y))?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = [31, if leap {29} else {28},31,30,31,30,31,31,30,31,30,31][month as usize - 1];
    if day < 1 || day > max_day { return None; }
    let time: Vec<_> = fields[4].split(':').map(str::parse::<u64>).collect::<Result<_,_>>().ok()?;
    if time.len() != 3 || time[0] > 23 || time[1] > 59 || time[2] > 59 { return None; }
    let adjusted = year - i64::from(month <= 2);
    let era = adjusted / 400;
    let yoe = adjusted - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    u64::try_from(days).ok()?.checked_mul(86400)?.checked_add(time[0]*3600 + time[1]*60 + time[2])
}
