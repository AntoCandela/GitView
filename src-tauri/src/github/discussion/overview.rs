//! Normalizes overview independently of paged metadata or discussion connection failures.
use super::fields::*;
use super::*;
pub(super) fn normalize(
    body: &Value,
    identity: &PrIdentity,
    revision: u64,
) -> Result<NormalizedOverview, Failure> {
    let (repo, pull) = envelope(body, identity, None)?;
    let base_repository = repository(repo)?;
    if !pull["repository"].is_null()
        && repository_id(&pull["repository"])? != identity.base_repository_id
    {
        return Err(PrCode::InvalidOutput.failure());
    }
    let version = version(pull)?;
    let overview = Overview {
        number: identity.number,
        url: format!("{}/pull/{}", base_repository.url, identity.number),
        base_repository,
        head_repository: if pull["headRepository"].is_null() {
            None
        } else {
            Some(repository(&pull["headRepository"])?)
        },
        head_ref: optional(&pull["headRefName"], 512)?,
        head_oid: version.head_oid.clone(),
        base_ref: text(&pull["baseRefName"], 512)?,
        base_oid: version.base_oid.clone(),
        title: text(&pull["title"], 4096)?,
        body: prose(&pull["body"]),
        author: actor(&pull["author"])?,
        lifecycle: version.lifecycle,
        draft: pull["isDraft"]
            .as_bool()
            .ok_or_else(|| PrCode::InvalidOutput.failure())?,
        created_at: timestamp(&pull["createdAt"])?
            .ok_or_else(|| PrCode::InvalidOutput.failure())?,
        updated_at: version.updated_at.clone(),
        closed_at: timestamp(&pull["closedAt"])?,
        merged_at: timestamp(&pull["mergedAt"])?,
        counts: PrCounts {
            commits: count(&pull["commits"]["totalCount"])?,
            files: count(&pull["changedFiles"])?,
            additions: count(&pull["additions"])?,
            deletions: count(&pull["deletions"])?,
        },
        review_decision: match pull["reviewDecision"].as_str() {
            Some("APPROVED") => Some(ReviewDecision::Approved),
            Some("CHANGES_REQUESTED") => Some(ReviewDecision::ChangesRequested),
            Some("REVIEW_REQUIRED") => Some(ReviewDecision::ReviewRequired),
            _ => None,
        },
        reviewers: limited(revision),
        labels: limited(revision),
    };
    Ok(NormalizedOverview {
        overview,
        version,
        grants: vec![],
    })
}
