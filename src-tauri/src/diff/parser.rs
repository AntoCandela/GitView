//! Validates the complete single-file unified patch before publishing any hunk.

use super::{unavailable, unsupported, LineKind, ReviewErrorCode, ReviewFailure, TextHunk, TextLine, UnsupportedReason, CONTENT_LIMIT};
use crate::git::process::ProbeDeadline;

pub(super) const MAX_LINES: usize = 32_768;
const MAX_HUNKS: usize = 1024;

pub(super) fn parse(bytes: &[u8], deadline: ProbeDeadline) -> Result<Vec<TextHunk>, ReviewFailure> {
    deadline.check().map_err(super::process_error)?;
    if bytes.len() > CONTENT_LIMIT { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
    if bytes.is_empty() { return Ok(Vec::new()); }
    let text = std::str::from_utf8(bytes).map_err(|_| unsupported(UnsupportedReason::UnsupportedEncoding))?;
    let text = text.strip_suffix('\n').ok_or_else(invalid)?;
    let mut lines = text.split('\n').peekable();
    if lines.next() != Some("diff --git from to") { return Err(invalid()); }
    let index = lines.next().and_then(|line| line.strip_prefix("index ")).ok_or_else(invalid)?;
    let (hashes, mode) = index.split_once(' ').ok_or_else(invalid)?;
    let (old_hash, new_hash) = hashes.split_once("..").ok_or_else(invalid)?;
    if mode != "100644" || ![old_hash, new_hash].iter().all(|hash| {
        (4..=64).contains(&hash.len()) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) { return Err(invalid()); }
    if lines.next() != Some("--- from") || lines.next() != Some("+++ to") { return Err(invalid()); }
    let mut hunks = Vec::new();
    let mut total_lines = 0;
    let mut old_end = 0u32;
    let mut new_end = 0u32;
    let mut old_terminal = false;
    let mut new_terminal = false;
    while let Some(header) = lines.next() {
        deadline.check().map_err(super::process_error)?;
        if hunks.len() == MAX_HUNKS { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
        let header = header.strip_prefix("@@ -").ok_or_else(invalid)?;
        let (ranges, label) = header.split_once(" @@").ok_or_else(invalid)?;
        if !label.is_empty() && !label.starts_with(' ') { return Err(invalid()); }
        let (old_range, new_range) = ranges.split_once(" +").ok_or_else(invalid)?;
        let (old_start, old_count) = range(old_range)?;
        let (new_start, new_count) = range(new_range)?;
        if (old_count > 0 && old_start == 0) || (new_count > 0 && new_start == 0)
            || old_start < old_end || new_start < new_end || (old_count == 0 && new_count == 0) {
            return Err(invalid());
        }
        old_end = old_start.checked_add(old_count).ok_or_else(invalid)?;
        new_end = new_start.checked_add(new_count).ok_or_else(invalid)?;
        let mut hunk = TextHunk { old_start, old_count, new_start, new_count, lines: Vec::new() };
        let mut old_seen = 0u32;
        let mut new_seen = 0u32;
        let mut changed = false;
        while let Some(line) = lines.peek().copied() {
            if line.starts_with("@@ ") { break; }
            lines.next();
            if line == "\\ No newline at end of file" {
                let previous = hunk.lines.last_mut().ok_or_else(invalid)?;
                if previous.no_final_newline { return Err(invalid()); }
                previous.no_final_newline = true;
                if previous.kind != LineKind::Addition { old_terminal = true; }
                if previous.kind != LineKind::Removal { new_terminal = true; }
                continue;
            }
            let (kind, content) = match line.as_bytes().first() {
                Some(b' ') => (LineKind::Context, &line[1..]),
                Some(b'+') => (LineKind::Addition, &line[1..]),
                Some(b'-') => (LineKind::Removal, &line[1..]),
                _ => return Err(invalid()),
            };
            if kind != LineKind::Addition {
                if old_terminal { return Err(invalid()); }
                old_seen = old_seen.checked_add(1).ok_or_else(invalid)?;
            }
            if kind != LineKind::Removal {
                if new_terminal { return Err(invalid()); }
                new_seen = new_seen.checked_add(1).ok_or_else(invalid)?;
            }
            if old_seen > old_count || new_seen > new_count { return Err(invalid()); }
            changed |= kind != LineKind::Context;
            total_lines += 1;
            if total_lines > MAX_LINES { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
            hunk.lines.push(TextLine { kind, text: content.to_owned(), no_final_newline: false });
        }
        if old_seen != old_count || new_seen != new_count || !changed { return Err(invalid()); }
        hunks.push(hunk);
    }
    if hunks.is_empty() { return Err(invalid()); }
    deadline.check().map_err(super::process_error)?;
    Ok(hunks)
}

fn range(text: &str) -> Result<(u32, u32), ReviewFailure> {
    let (start, count) = text.split_once(',').unwrap_or((text, "1"));
    Ok((number(start)?, number(count)?))
}
fn number(text: &str) -> Result<u32, ReviewFailure> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) { return Err(invalid()); }
    text.parse().map_err(|_| invalid())
}
fn invalid() -> ReviewFailure { unavailable(ReviewErrorCode::InvalidOutput) }
