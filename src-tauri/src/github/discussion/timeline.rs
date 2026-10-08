//! Preserves provider ordering and event meaning without turning commit dates into push times.
use super::fields::*;
use super::*;

pub(super) fn opening(
    pull: &Value,
    request: &PageAuthority,
    grants: &mut Vec<Grant>,
) -> Result<TimelineItem, Failure> {
    Ok(TimelineItem {
        id: format!(
            "opened:{}:{}",
            request.identity.base_repository_id, request.identity.number
        ),
        kind: TimelineKind::Opened,
        occurred_at: timestamp(&pull["createdAt"])?,
        actor: actor(&pull["author"])?,
        provider_order: 0,
        details: TimelineDetails::Activity { body: Prose::Empty },
        link_id: link(&pull["url"], &request.identity, grants),
    })
}
pub(super) fn event(
    node: &Value,
    id: String,
    order: u64,
    request: &PageAuthority,
    grants: &mut Vec<Grant>,
) -> Result<(TimelineItem, bool), Failure> {
    let kind = node["__typename"].as_str().unwrap_or("");
    let mut occurred_at = timestamp(&node["createdAt"])?;
    let mut event_actor = actor(&node["actor"])?;
    let mut unsupported = false;
    let (kind, details) = match kind {
        "IssueComment" => {
            let comment = super::threads::comment(node, CommentSource::Issue)?;
            event_actor = comment.author.clone();
            occurred_at = comment.created_at.clone();
            (TimelineKind::Comment, TimelineDetails::Comment { comment })
        }
        "PullRequestReview" => {
            event_actor = actor(&node["author"])?;
            occurred_at = timestamp(&node["submittedAt"])?;
            let state = text(&node["state"], 64)?;
            let kind = if matches!(
                state.as_str(),
                "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED" | "DISMISSED"
            ) {
                TimelineKind::ReviewSubmitted
            } else {
                unsupported = true;
                TimelineKind::Unsupported
            };
            (
                kind,
                TimelineDetails::Review {
                    review_id: id.clone(),
                    state,
                    body: prose(&node["body"]),
                    thread_ids: vec![],
                },
            )
        }
        "PullRequestCommit" => {
            occurred_at = None;
            event_actor = None;
            let commit = &node["commit"];
            (
                TimelineKind::Commit,
                TimelineDetails::Commit {
                    commit_oid: optional_oid(&commit["oid"])?
                        .ok_or_else(|| PrCode::InvalidOutput.failure())?,
                    authored_at: timestamp(&commit["authoredDate"])?,
                    committed_at: timestamp(&commit["committedDate"])?,
                },
            )
        }
        "ConvertToDraftEvent" => (
            TimelineKind::DraftChanged,
            TimelineDetails::DraftChanged { draft: true },
        ),
        "ReadyForReviewEvent" => (
            TimelineKind::DraftChanged,
            TimelineDetails::DraftChanged { draft: false },
        ),
        "HeadRefForcePushedEvent" | "BaseRefForcePushedEvent" => (
            TimelineKind::ForcePushed,
            TimelineDetails::ForcePushed {
                before_oid: optional_oid(&node["beforeCommit"]["oid"])?,
                after_oid: optional_oid(&node["afterCommit"]["oid"])?,
                is_base: kind == "BaseRefForcePushedEvent",
            },
        ),
        "BaseRefChangedEvent" => (
            TimelineKind::BaseChanged,
            TimelineDetails::BaseChanged {
                previous_ref: optional(&node["previousRefName"], 512)?,
                current_ref: optional(&node["currentRefName"], 512)?,
            },
        ),
        "ReviewRequestedEvent" | "ReviewRequestRemovedEvent" => {
            let removed = kind == "ReviewRequestRemovedEvent";
            (
                if removed {
                    TimelineKind::ReviewRemoved
                } else {
                    TimelineKind::ReviewRequested
                },
                TimelineDetails::ReviewRequest {
                    reviewer: reviewer(&node["requestedReviewer"])?,
                    removed,
                },
            )
        }
        "ReviewDismissedEvent" => {
            let details = if let Some(review_id) = optional(&node["review"]["id"], MAX_ID)? {
                TimelineDetails::Review {
                    review_id,
                    state: text(&node["previousReviewState"], 64)?,
                    body: prose(&node["dismissalMessage"]),
                    thread_ids: vec![],
                }
            } else {
                TimelineDetails::Activity {
                    body: prose(&node["dismissalMessage"]),
                }
            };
            (TimelineKind::ReviewDismissed, details)
        }
        "ClosedEvent" => (
            TimelineKind::Closed,
            TimelineDetails::Activity { body: Prose::Empty },
        ),
        "ReopenedEvent" => (
            TimelineKind::Reopened,
            TimelineDetails::Activity { body: Prose::Empty },
        ),
        "MergedEvent" => {
            let details = if let Some(commit_oid) = optional_oid(&node["commit"]["oid"])? {
                TimelineDetails::Commit {
                    commit_oid,
                    authored_at: None,
                    committed_at: None,
                }
            } else {
                TimelineDetails::Activity { body: Prose::Empty }
            };
            (TimelineKind::Merged, details)
        }
        _ => {
            unsupported = true;
            (
                TimelineKind::Unsupported,
                TimelineDetails::Activity { body: Prose::Empty },
            )
        }
    };
    Ok((
        TimelineItem {
            id,
            kind,
            occurred_at,
            actor: event_actor,
            provider_order: order,
            details,
            link_id: link(&node["url"], &request.identity, grants),
        },
        unsupported,
    ))
}
