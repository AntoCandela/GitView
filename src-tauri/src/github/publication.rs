//! Validates that public result references belong to the authority issued for their operation.
use super::{model::*, service::Resource};

type Lookup<'a> = dyn Fn(&str) -> Option<Resource> + 'a;

pub(super) fn valid(request: &PrRequest, result: &PrResult, lookup: &Lookup<'_>) -> bool {
    match result {
        PrResult::Failure(_) => true,
        PrResult::Success(success) => match success {
            PrSuccess::Ready | PrSuccess::Opened | PrSuccess::Blocked => true,
            PrSuccess::Association { observation } => association(observation, lookup),
            PrSuccess::Snapshot { snapshot } => cursor(snapshot.overview.reviewers.next_cursor.as_deref(), None, Some(CollectionKind::Reviewers), None, lookup)
                && cursor(snapshot.overview.labels.next_cursor.as_deref(), None, Some(CollectionKind::Labels), None, lookup),
            PrSuccess::Comparison { comparison } => valid_comparison(comparison, lookup),
            PrSuccess::Resolved { comparison, file_id, line, .. } => *line > 0 && valid_comparison(comparison, lookup)
                && file(file_id, &comparison.comparison_id, lookup),
            PrSuccess::Fallback { link_id, .. } => link(link_id.as_deref(), lookup),
            PrSuccess::Files { collection } => if let PrRequest::FilesPage { comparison_id, .. } = request {
                files(collection, comparison_id, lookup)
            } else { false },
            PrSuccess::File { comparison_id, file_id, .. } => matches!(request, PrRequest::File { comparison_id: expected_comparison, file_id: expected_file }
                if expected_comparison == comparison_id && expected_file == file_id) && file(file_id, comparison_id, lookup),
            PrSuccess::Page { collection } => if let PrRequest::Page { collection: kind, thread_id, .. } = request {
                cursor(collection.next_cursor.as_deref(), None, Some(*kind), thread_id.as_deref(), lookup)
                    && collection.items.iter().all(|item| valid_item(item, *kind, lookup))
            } else { false },
            PrSuccess::Chosen { .. } | PrSuccess::Released => false,
        },
    }
}
fn cursor(id: Option<&str>, comparison: Option<&str>, collection: Option<CollectionKind>, thread: Option<&str>, lookup: &Lookup<'_>) -> bool {
    id.is_none_or(|id| matches!(lookup(id), Some(Resource::Cursor { comparison_id, collection: c, thread_id, .. })
        if comparison_id.as_deref() == comparison && c == collection && thread_id.as_deref() == thread))
}
fn file(id: &str, comparison: &str, lookup: &Lookup<'_>) -> bool {
    matches!(lookup(id), Some(Resource::File { comparison_id, .. }) if comparison_id == comparison)
}
fn link(id: Option<&str>, lookup: &Lookup<'_>) -> bool { id.is_none_or(|id| matches!(lookup(id), Some(Resource::Link { .. }))) }
fn files(collection: &Collection<PrFile>, comparison: &str, lookup: &Lookup<'_>) -> bool {
    cursor(collection.next_cursor.as_deref(), Some(comparison), None, None, lookup)
        && collection.items.iter().all(|item| file(&item.file_id, comparison, lookup))
}
fn valid_comparison(comparison: &Comparison, lookup: &Lookup<'_>) -> bool {
    matches!(lookup(&comparison.comparison_id), Some(Resource::Comparison)) && files(&comparison.files, &comparison.comparison_id, lookup)
        && match comparison.source {
            ComparisonSource::LocalGit => comparison.full_content && comparison.files.next_cursor.is_none(),
            ComparisonSource::GithubPatch => !comparison.full_content,
        }
}
fn valid_item(item: &Item, collection: CollectionKind, lookup: &Lookup<'_>) -> bool {
    match (collection, item) {
        (CollectionKind::Commits, Item::Commit { commit_id, oid, parent_count, .. }) => matches!(lookup(commit_id), Some(Resource::Commit { authority, parent_count: count }) if count == *parent_count && authority.oid == *oid),
        (CollectionKind::Timeline, Item::Timeline { event }) => link(event.link_id.as_deref(), lookup),
        (CollectionKind::Threads, Item::Thread { thread }) => valid_thread_anchor(thread, lookup)
            && cursor(thread.comments.next_cursor.as_deref(), None, Some(CollectionKind::ThreadComments), Some(&thread.id), lookup)
            && link(thread.link_id.as_deref(), lookup),
        (CollectionKind::ThreadComments, Item::Comment { .. }) | (CollectionKind::Reviewers, Item::Reviewer { .. }) | (CollectionKind::Labels, Item::Label { .. }) => true,
        _ => false,
    }
}

fn association(observation: &Association, lookup: &Lookup<'_>) -> bool {
    if !matches!(lookup(&observation.association_id), Some(Resource::Association { .. })) { return false; }
    let candidates: Vec<_> = observation.candidates.iter().chain(&observation.historical).collect();
    let unique: std::collections::HashSet<_> = candidates.iter().map(|c| &c.candidate_id).collect();
    unique.len() == candidates.len()
        && observation.selected_candidate_id.as_ref().is_none_or(|id| unique.contains(id))
        && match observation.state {
            AssociationState::Single => observation.complete && observation.failure.is_none() && observation.candidates.len() == 1,
            AssociationState::None => observation.complete && observation.failure.is_none() && observation.candidates.is_empty(),
            _ => true,
        }
        && candidates.iter().all(|public| matches!(lookup(&public.candidate_id),
            Some(Resource::Candidate { association_id, candidate }) if association_id == observation.association_id
                && candidate.identity.number == public.number && candidate.identity.base_repository_id == public.base_repository.id
                && candidate.base_ref == public.base_ref && candidate.head_ref == public.head_ref
                && candidate.head_repository.as_ref().map(|r| &r.id) == public.head_repository.as_ref().map(|r| &r.id)))
}

// An anchor must describe this exact thread, not merely another grant in its PR session.
fn valid_thread_anchor(thread: &Thread, lookup: &Lookup<'_>) -> bool {
    let Some(Resource::Thread { authority: owner }) = lookup(&thread.id) else { return false; };
    thread.anchor_id.as_ref().is_none_or(|id| {
        let Some(Resource::Anchor { authority }) = lookup(id) else { return false; };
        authority.thread_provider_id == owner.provider_id
            && authority.current.iter().chain(&authority.original).all(|position|
                thread.path.as_deref() == Some(position.path.as_str()) && thread.side == Some(position.side))
            && authority.current.as_ref().is_none_or(|position|
                thread.current_commit_oid.as_deref() == Some(position.commit_oid.as_str())
                    && thread.line == Some(position.line) && thread.start_line == position.start_line)
            && authority.original.as_ref().is_none_or(|position|
                thread.original_commit_oid.as_deref() == Some(position.commit_oid.as_str()))
    })
}
