//! Captures provider-owned thread context; comparison navigation must still verify its endpoints.
use super::fields::*;
use super::*;
use crate::github::service::Resource;

pub(super) fn comment(value: &Value, source_kind: CommentSource) -> Result<Comment, Failure> {
    Ok(Comment {
        id: text(&value["id"], MAX_ID)?,
        author: actor(&value["author"])?,
        created_at: timestamp(&value["createdAt"])?,
        updated_at: timestamp(&value["updatedAt"])?,
        body: prose(&value["body"]),
        reply_to_id: optional(&value["replyTo"]["id"], MAX_ID)?,
        source_kind,
    })
}
fn boolean(value: &Value) -> Result<Option<bool>, Failure> {
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_bool()
            .map(Some)
            .ok_or_else(|| PrCode::InvalidOutput.failure())
    }
}
fn line(value: &Value) -> Result<Option<u32>, Failure> {
    match count(value)? {
        None => Ok(None),
        Some(value) => u32::try_from(value)
            .ok()
            .filter(|line| *line > 0)
            .map(Some)
            .ok_or_else(|| PrCode::InvalidOutput.failure()),
    }
}
fn side(value: &Value) -> Option<Side> {
    match value.as_str() {
        Some("LEFT") => Some(Side::Old),
        Some("RIGHT") => Some(Side::New),
        _ => None,
    }
}
fn position(
    commit_oid: Option<String>,
    path: Option<&str>,
    side: Option<Side>,
    start_line: Option<u32>,
    line: Option<u32>,
) -> Option<AnchorPosition> {
    let position = AnchorPosition {
        commit_oid: commit_oid?,
        path: path?.into(),
        side: side?,
        start_line,
        line: line?,
    };
    position.validate().then_some(position)
}
pub(super) fn thread(
    value: &Value,
    request: &PageAuthority,
    revision: u64,
    grants: &mut Vec<Grant>,
) -> Result<Thread, Failure> {
    let provider_id = text(&value["id"], MAX_ID)?;
    let grant = Grant::new(Resource::Thread {
        authority: Box::new(ThreadAuthority {
            identity: request.identity.clone(),
            version: request.version.clone(),
            provider_id: provider_id.clone(),
        }),
    });
    let id = grant.id().to_owned();
    grants.push(grant);
    let nested = PageAuthority {
        identity: request.identity.clone(),
        version: request.version.clone(),
        collection: CollectionKind::ThreadComments,
        thread_provider_id: Some(provider_id.clone()),
        thread_id: Some(id.clone()),
    };
    let comments = if value["comments"].is_object() {
        match super::page::connection_page(
            &value["comments"],
            &Value::Null,
            &nested,
            None,
            revision,
        ) {
            Ok(page) => {
                grants.extend(page.grants);
                Collection {
                    items: page
                        .collection
                        .items
                        .into_iter()
                        .filter_map(|item| match item {
                            Item::Comment { comment } => Some(comment),
                            _ => None,
                        })
                        .collect(),
                    total_count: page.collection.total_count,
                    next_cursor: page.collection.next_cursor,
                    completeness: page.collection.completeness,
                    limit_reason: page.collection.limit_reason,
                    observed_revision: revision,
                }
            }
            Err(_) => limited(revision),
        }
    } else {
        limited(revision)
    };
    // Replies cannot invent root-comment anchor evidence when the initial comment is unavailable.
    let root = value["comments"]["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| node["replyTo"].is_null()));
    let root = root.unwrap_or(&Value::Null);
    let path = optional(&value["path"], 4096)?;
    let original_commit_oid = optional_oid(&root["originalCommit"]["oid"])?;
    let current_commit_oid = optional_oid(&root["commit"]["oid"])?;
    let diff_excerpt = prose(&root["diffHunk"]);
    let resolved = boolean(&value["isResolved"])?;
    let outdated = boolean(&value["isOutdated"])?;
    let diff_side = side(&value["diffSide"]);
    let start_line = line(&value["startLine"])?;
    let current_line = line(&value["line"])?;
    let original_start = line(&value["originalStartLine"])?;
    let original_line = line(&value["originalLine"])?;
    let safe_path = path
        .as_deref()
        .filter(|path| root["path"].as_str() == Some(*path));
    let same_side = value["startDiffSide"].is_null() || value["startDiffSide"] == value["diffSide"];
    let line_subject = value["subjectType"].as_str() != Some("FILE") && same_side;
    let current =
        if line_subject && outdated != Some(true) && current_commit_oid == request.version.head_oid
        {
            position(
                current_commit_oid.clone(),
                safe_path,
                diff_side,
                start_line,
                current_line,
            )
        } else {
            None
        };
    let original = if line_subject {
        position(
            original_commit_oid.clone(),
            safe_path,
            diff_side,
            original_start,
            original_line,
        )
    } else {
        None
    };
    let url = canonical_link(&root["url"], &request.identity);
    let anchor_id = if current.is_some() || original.is_some() {
        let authority = AnchorAuthority {
            identity: request.identity.clone(),
            version: request.version.clone(),
            thread_provider_id: provider_id,
            current,
            original,
            excerpt: diff_excerpt.clone(),
            url,
        };
        let grant = Grant::new(Resource::Anchor {
            authority: Box::new(authority),
        });
        let anchor_id = grant.id().to_owned();
        grants.push(grant);
        Some(anchor_id)
    } else {
        None
    };
    let link_id = link(&root["url"], &request.identity, grants);
    Ok(Thread {
        id,
        anchor_id,
        review_id: optional(&root["pullRequestReview"]["id"], MAX_ID)?,
        resolved,
        outdated,
        path,
        original_commit_oid,
        current_commit_oid,
        side: diff_side,
        start_line,
        line: current_line,
        diff_excerpt,
        comments,
        link_id,
    })
}
