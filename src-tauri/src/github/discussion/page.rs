//! Keeps continuation, deduplication and resource limits separate for every provider connection.
use super::fields::*;
use super::*;
use crate::github::service::{CursorTarget, Resource};

pub(super) fn normalize(
    body: &Value,
    request: &PageAuthority,
    cursor: Option<&CursorAuthority>,
    revision: u64,
) -> Result<NormalizedPage, Failure> {
    if !request.validate() {
        return Err(PrCode::InvalidOutput.failure());
    }
    if let Some(cursor) = cursor {
        if !cursor.validate()
            || cursor.page.identity != request.identity
            || cursor.page.collection != request.collection
            || cursor.page.thread_provider_id != request.thread_provider_id
            || cursor.page.thread_id != request.thread_id
            || !same_version(&cursor.page.version, &request.version, true)
        {
            return Err(PrCode::StaleCursor.failure());
        }
    }
    let (_, pull) = envelope(
        body,
        &request.identity,
        request.thread_provider_id.as_deref(),
    )?;
    let current = version(pull)?;
    if !same_version(
        &current,
        &request.version,
        request.collection == CollectionKind::Commits,
    ) {
        return Err(PrCode::ChangedSnapshot.failure());
    }
    let connection = match request.collection {
        CollectionKind::ThreadComments => &body["data"]["node"]["comments"],
        CollectionKind::Commits => &pull["commits"],
        CollectionKind::Timeline => &pull["timelineItems"],
        CollectionKind::Threads => &pull["reviewThreads"],
        CollectionKind::Reviewers => &pull["reviewRequests"],
        CollectionKind::Labels => &pull["labels"],
    };
    connection_page(connection, pull, request, cursor, revision)
}
pub(super) fn connection_page(
    connection: &Value,
    pull: &Value,
    request: &PageAuthority,
    cursor: Option<&CursorAuthority>,
    revision: u64,
) -> Result<NormalizedPage, Failure> {
    let nodes = connection["nodes"]
        .as_array()
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    if nodes.len() > 100 {
        return Err(PrCode::ResourceLimit.failure());
    }
    let has_next = connection["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let next = if has_next {
        Some(text(&connection["pageInfo"]["endCursor"], 4096)?)
    } else {
        None
    };
    if next
        .as_ref()
        .is_some_and(|next| cursor.is_some_and(|old| old.cursor == *next))
    {
        return Err(PrCode::InvalidOutput.failure());
    }
    let mut total_count = count(&connection["totalCount"])?;
    let mut seen = cursor.map(|c| c.seen_ids.clone()).unwrap_or_default();
    let mut provider_order = cursor.map(|c| c.provider_order).unwrap_or(0);
    let previous_items = cursor.map(|c| c.items as usize).unwrap_or(0);
    let pages = cursor.map(|c| c.pages).unwrap_or(0) + 1;
    if previous_items >= MAX_ITEMS || pages > MAX_PAGES {
        return Err(PrCode::ResourceLimit.failure());
    }
    let mut items = Vec::new();
    let mut grants = Vec::new();
    let mut provider_limited = cursor.is_some_and(|c| c.provider_limited);
    let mut resource_limited = false;
    if request.collection == CollectionKind::Timeline && !pull["createdAt"].is_null() {
        total_count = total_count.and_then(|count| count.checked_add(1));
        if cursor.is_none() {
            let event = super::timeline::opening(pull, request, &mut grants)?;
            seen.push(event.id.clone());
            items.push(Item::Timeline { event });
            provider_order += 1;
        }
    }
    let mut consumed = 0;
    for node in nodes {
        if previous_items + consumed >= MAX_ITEMS || seen.len() >= MAX_ITEMS {
            resource_limited = true;
            break;
        }
        let id = match node["id"].as_str() {
            Some(_) => text(&node["id"], MAX_ID)?,
            None if request.collection == CollectionKind::Timeline => {
                format!("unsupported:{provider_order}")
            }
            _ => return Err(PrCode::InvalidOutput.failure()),
        };
        let order = provider_order;
        provider_order += 1;
        consumed += 1;
        if seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        let item = match request.collection {
            CollectionKind::Commits => Some(commit(node, request, &mut grants)?),
            CollectionKind::Timeline => {
                let (event, unsupported) =
                    super::timeline::event(node, id, order, request, &mut grants)?;
                provider_limited |= unsupported;
                Some(Item::Timeline { event })
            }
            CollectionKind::Threads => Some(Item::Thread {
                thread: super::threads::thread(node, request, revision, &mut grants)?,
            }),
            CollectionKind::ThreadComments => Some(Item::Comment {
                comment: super::threads::comment(node, CommentSource::Thread)?,
            }),
            CollectionKind::Reviewers => match reviewer(&node["requestedReviewer"])? {
                Some(reviewer) => Some(Item::Reviewer { reviewer }),
                None => {
                    provider_limited = true;
                    None
                }
            },
            CollectionKind::Labels => Some(Item::Label {
                label: Label {
                    name: text(&node["name"], 512)?,
                    color: optional(&node["color"], 64)?,
                },
            }),
        };
        if let Some(item) = item {
            items.push(item);
        }
    }
    let item_count = previous_items + consumed;
    resource_limited |=
        has_next && (pages >= MAX_PAGES || item_count >= MAX_ITEMS || seen.len() >= MAX_ITEMS);
    let mut next_cursor = None;
    if !resource_limited {
        if let Some(cursor) = next {
            let authority = CursorAuthority {
                page: request.clone(),
                cursor,
                provider_order,
                seen_ids: seen,
                pages,
                items: item_count as u32,
                provider_limited,
            };
            if !authority.validate() {
                return Err(PrCode::ResourceLimit.failure());
            }
            let grant = Grant::new(Resource::Cursor {
                comparison_id: None,
                collection: Some(request.collection),
                thread_id: request.thread_id.clone(),
                target: CursorTarget::Discussion(Box::new(authority)),
            });
            next_cursor = Some(grant.id().to_owned());
            grants.push(grant);
        }
    }
    let (completeness, limit_reason) = if resource_limited {
        (Completeness::Limited, Some(LimitReason::Resource))
    } else if provider_limited {
        (Completeness::Limited, Some(LimitReason::Provider))
    } else if has_next {
        (Completeness::More, None)
    } else {
        (Completeness::Complete, None)
    };
    Ok(NormalizedPage {
        collection: Collection {
            items,
            total_count,
            next_cursor,
            completeness,
            limit_reason,
            observed_revision: revision,
        },
        grants,
    })
}
fn commit(node: &Value, request: &PageAuthority, grants: &mut Vec<Grant>) -> Result<Item, Failure> {
    let value = &node["commit"];
    let commit_oid = optional_oid(&value["oid"])?.ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let parent_count = count(&value["parents"]["totalCount"])?
        .and_then(|c| u32::try_from(c).ok())
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let parent_nodes = value["parents"]["nodes"]
        .as_array()
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    if parent_nodes.len() > 100 {
        return Err(PrCode::ResourceLimit.failure());
    }
    let parents: Vec<String> = parent_nodes
        .iter()
        .map(|p| {
            optional_oid(&p["oid"])
                .and_then(|oid| oid.ok_or_else(|| PrCode::InvalidOutput.failure()))
        })
        .collect::<Result<_, _>>()?;
    if parents.len() > parent_count as usize {
        return Err(PrCode::InvalidOutput.failure());
    }
    let has_next = value["parents"]["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let authority = CommitAuthority {
        identity: request.identity.clone(),
        version: request.version.clone(),
        oid: commit_oid.clone(),
        parents_complete: !has_next && parents.len() == parent_count as usize,
        parents,
    };
    let grant = Grant::new(Resource::Commit {
        authority: Box::new(authority),
        parent_count,
    });
    let commit_id = grant.id().to_owned();
    grants.push(grant);
    let message = value["message"]
        .as_str()
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let title = message.lines().next().unwrap_or("");
    if title.len() > 4096 {
        return Err(PrCode::ResourceLimit.failure());
    }
    Ok(Item::Commit {
        commit_id,
        oid: commit_oid,
        title: title.into(),
        parent_count,
        authored_at: timestamp(&value["authoredDate"])?,
        committed_at: timestamp(&value["committedDate"])?,
    })
}
