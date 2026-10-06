//! Fetches only a configured tracking ref and pins independent merge-base comparisons.

use super::*;
use crate::git::process::ProbeDeadline;
use reader::{required, valid_ref_name};

/// A comparison is immutable; its token, rather than either displayed OID, grants authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamRange { pub token: String, pub base_oid: String, pub tip_oid: String }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamState { Ready, NoUpstream, Detached, Unborn, Unavailable }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamFreshness { Fresh, Stale, Unavailable }
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamSummary {
    pub state: UpstreamState, pub freshness: UpstreamFreshness,
    pub branch: Option<String>, pub upstream: Option<String>,
    pub ahead: u64, pub behind: u64,
    pub incoming: Option<UpstreamRange>, pub outgoing: Option<UpstreamRange>,
}
impl UpstreamSummary {
    pub(super) fn empty(state: UpstreamState, branch: Option<String>) -> Self {
        Self { state, freshness: UpstreamFreshness::Unavailable, branch, upstream: None, ahead: 0, behind: 0, incoming: None, outgoing: None }
    }
}

pub(super) struct Tracking { branch: String, target: String, fetched: bool }

/// Resolve configuration through Git's ref atoms, then override its fetch mappings entirely.
/// Only refs/remotes may be updated; tags, FETCH_HEAD, submodules and maintenance are disabled.
pub(super) async fn fetch(process: &GitProcess, context: &SelectedContext, viewed: Option<&str>) -> Result<Option<Tracking>, HistoryErrorCode> {
    let deadline = ProbeDeadline::new();
    let branch = match viewed {
        Some(branch) => branch.to_owned(),
        None => {
            let output = reader::run(process, &context.root, &["symbolic-ref", "--quiet", "HEAD"], None, deadline).await?;
            if !output.status.success() { return Ok(None); }
            let name = line(&output.stdout)?;
            name.strip_prefix("refs/heads/").ok_or(HistoryErrorCode::InvalidOutput)?.to_owned()
        }
    };
    let reference = format!("refs/heads/{branch}");
    if !valid_ref_name(&reference) { return Err(HistoryErrorCode::InvalidOutput); }
    let bytes = required(process, &context.root, &["for-each-ref", "--format=%(refname)%00%(upstream)%00%(upstream:remotename)%00%(upstream:remoteref)", &reference], None, deadline).await?;
    // for-each-ref patterns also match descendants: only the exact selected branch is authority.
    let text = std::str::from_utf8(&bytes).map_err(|_| HistoryErrorCode::InvalidOutput)?;
    let Some(fields) = text.lines().map(|row| row.split('\0').collect::<Vec<_>>()).find(|row| row.first() == Some(&reference.as_str())) else { return Ok(None); };
    if fields.len() != 4 { return Err(HistoryErrorCode::InvalidOutput); }
    let (target, remote, source) = (fields[1], fields[2], fields[3]);
    if target.is_empty() { return Ok(None); }
    // A local-dot upstream needs no network; it remains read-only and is resolved below.
    if remote == "." && target.starts_with("refs/heads/") && valid_ref_name(target) {
        return Ok(Some(Tracking { branch, target: target.to_owned(), fetched: true }));
    }
    if !target.starts_with("refs/remotes/") || !source.starts_with("refs/heads/")
        || !valid_ref_name(target) || !valid_ref_name(source) || remote.starts_with('-') || !valid_ref_name(remote) {
        return Err(HistoryErrorCode::InvalidOutput);
    }
    let symbolic = reader::run(process, &context.root, &["symbolic-ref", "--quiet", target], None, deadline).await?;
    if symbolic.status.code() != Some(1) { return Err(HistoryErrorCode::InvalidOutput); }
    let refspec = format!("+{source}:{target}");
    let fetched = reader::run(process, &context.root, &[
        "-c", "credential.interactive=false", "-c", "core.hooksPath=/dev/null", "fetch", "--no-tags", "--no-prune", "--no-prune-tags", "--no-recurse-submodules",
        "--no-write-fetch-head", "--no-auto-maintenance", "--refmap=", "--", remote, &refspec,
    ], None, ProbeDeadline::new()).await.is_ok_and(|output| output.status.success());
    Ok(Some(Tracking { branch, target: target.to_owned(), fetched }))
}

pub(super) async fn summarize(process: &GitProcess, context: &SelectedContext, snapshot: &PinnedSnapshot, viewed: Option<&str>, tracking: Result<Option<Tracking>, HistoryErrorCode>, deadline: ProbeDeadline) -> UpstreamSummary {
    let branch = viewed.map(str::to_owned).or_else(|| snapshot.head.branch.clone());
    let mut summary = UpstreamSummary::empty(match snapshot.head.state {
        HeadState::Unborn if viewed.is_none() => UpstreamState::Unborn,
        HeadState::Detached if viewed.is_none() => UpstreamState::Detached,
        _ => UpstreamState::NoUpstream,
    }, branch);
    let tracking = match tracking {
        Ok(Some(tracking)) => tracking,
        Ok(None) => return summary,
        Err(_) => { summary.state = UpstreamState::Unavailable; return summary; }
    };
    if summary.branch.as_deref() != Some(tracking.branch.as_str()) {
        summary.state = UpstreamState::Unavailable;
        return summary;
    }
    summary.upstream = Some(tracking.target.strip_prefix("refs/remotes/").or_else(|| tracking.target.strip_prefix("refs/heads/")).unwrap_or(&tracking.target).to_owned());
    summary.state = UpstreamState::Unavailable;
    let local = snapshot.refs.iter().find(|r| r.kind == RefKind::LocalBranch && r.name == tracking.branch);
    let remote = snapshot.refs.iter().find(|r| match r.kind {
        RefKind::LocalBranch => tracking.target == format!("refs/heads/{}", r.name),
        RefKind::RemoteTracking => tracking.target == format!("refs/remotes/{}", r.name),
        RefKind::Tag => false,
    });
    let (Some(local), Some(remote)) = (local, remote) else { return summary; };
    summary.freshness = if tracking.fetched { UpstreamFreshness::Fresh } else { UpstreamFreshness::Stale };
    let result = comparisons(process, context, &local.commit_oid, &remote.commit_oid, deadline).await;
    if let Ok((ahead, behind, base)) = result {
        summary.ahead = ahead;
        summary.behind = behind;
        if let Some(base) = base.filter(|_| !snapshot.shallow) {
            summary.state = UpstreamState::Ready;
            let range = |tip: &str| UpstreamRange { token: Uuid::new_v4().to_string(), base_oid: base.clone(), tip_oid: tip.to_owned() };
            summary.incoming = (behind > 0).then(|| range(&remote.commit_oid));
            summary.outgoing = (ahead > 0).then(|| range(&local.commit_oid));
        }
    }
    summary
}
async fn comparisons(process: &GitProcess, context: &SelectedContext, local: &str, remote: &str, deadline: ProbeDeadline) -> Result<(u64, u64, Option<String>), HistoryErrorCode> {
    let range = format!("{local}...{remote}");
    let bytes = required(process, &context.root, &["rev-list", "--left-right", "--count", &range, "--"], None, deadline).await?;
    let counts = line(&bytes)?.split_whitespace().map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().map_err(|_| HistoryErrorCode::InvalidOutput)?;
    if counts.len() != 2 { return Err(HistoryErrorCode::InvalidOutput); }
    let output = reader::run(process, &context.root, &["merge-base", "--all", local, remote], None, deadline).await?;
    let base = if output.status.success() { reader::output_oid(&output.stdout).ok().map(str::to_owned) } else { None };
    Ok((counts[0], counts[1], base))
}
fn line(bytes: &[u8]) -> Result<&str, HistoryErrorCode> {
    std::str::from_utf8(bytes.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?).map_err(|_| HistoryErrorCode::InvalidOutput)
}
