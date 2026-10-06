//! Composes normalized review observations with shared demand and captured session authority.
use super::*;
use crate::github::{
    coordinator::{CachedPage, DemandScope, SnapshotVersion},
    discussion::{self, PageAuthority, PrVersion},
    model::*,
    service::CursorTarget,
    transport::{Connection, GhRead},
};

impl GithubProvider {
    pub(super) async fn review(&self, request: &ProviderRequest) -> Result<Publication, Failure> {
        let identity = request
            .identity
            .as_ref()
            .ok_or_else(|| PrCode::StaleContext.failure())?;
        let route = request
            .base_repository
            .as_ref()
            .ok_or_else(|| PrCode::StaleContext.failure())?;
        if route.id != identity.base_repository_id {
            return Err(PrCode::InvalidOutput.failure());
        }
        let capture = self.resolver.capture(&request.repository, None).await?;
        let scope = DemandScope {
            common_storage: capture.common_storage,
            config_fingerprint: capture.config_fingerprint,
            mapped_branch: None,
            identity: Some(identity.clone()),
            version: SnapshotVersion {
                revision: request.review.as_ref().map_or(0, |r| r.session.revision),
                base_oid: request
                    .review
                    .as_ref()
                    .and_then(|r| r.snapshot.base_oid.clone()),
                head_oid: request
                    .review
                    .as_ref()
                    .and_then(|r| r.snapshot.head_oid.clone()),
                source: None,
                base_kind: None,
            },
        };
        match &request.request {
            PrRequest::Open { .. } | PrRequest::Refresh { .. } => {
                self.overview(request, scope, route).await
            }
            PrRequest::Page {
                collection,
                thread_id,
                cursor,
                ..
            } => {
                let review = request
                    .review
                    .as_ref()
                    .ok_or_else(|| PrCode::StaleContext.failure())?;
                let thread_provider_id = if thread_id.is_some() {
                    Some(
                        request
                            .resources
                            .iter()
                            .find_map(|g| match &g.resource {
                                Resource::Thread { authority } => {
                                    Some(authority.provider_id.clone())
                                }
                                _ => None,
                            })
                            .ok_or_else(|| PrCode::StaleCursor.failure())?,
                    )
                } else {
                    None
                };
                let authority = PageAuthority {
                    identity: identity.clone(),
                    version: version(&review.snapshot),
                    collection: *collection,
                    thread_id: thread_id.clone(),
                    thread_provider_id,
                };
                let continuation = if cursor.is_some() {
                    Some(
                        request
                            .resources
                            .iter()
                            .find_map(|g| match &g.resource {
                                Resource::Cursor {
                                    target: CursorTarget::Discussion(authority),
                                    ..
                                } => Some(authority.as_ref()),
                                _ => None,
                            })
                            .ok_or_else(|| PrCode::StaleCursor.failure())?,
                    )
                } else {
                    None
                };
                let read = connection_read(
                    route,
                    identity.number,
                    *collection,
                    authority.thread_provider_id.clone(),
                    continuation.map(|c| c.cursor.clone()),
                )?;
                let page = self.demand(request, scope.clone(), read, false).await?;
                let normalized = discussion::normalize_page(
                    &page.body,
                    &authority,
                    continuation,
                    review.session.revision,
                )?;
                // Commit membership belongs to the captured PR version. A fresh trailing read
                // detects replacement during this batch before any new commit grants are issued.
                if *collection == CollectionKind::Commits {
                    let current = self
                        .demand(request, scope, overview_read(route, identity.number), true)
                        .await?;
                    let current = discussion::normalize_overview(
                        &current.body,
                        identity,
                        review.session.revision,
                    )?;
                    if !discussion::same_version(&authority.version, &current.version, true) {
                        return Err(PrCode::ChangedSnapshot.failure());
                    }
                }
                Ok(Publication {
                    result: PrSuccess::Page {
                        collection: normalized.collection,
                    }
                    .into(),
                    grants: normalized.grants,
                })
            }
            _ => Err(PrCode::IntegrationUnavailable.failure()),
        }
    }

    async fn overview(
        &self,
        request: &ProviderRequest,
        scope: DemandScope,
        route: &GithubRepository,
    ) -> Result<Publication, Failure> {
        let identity = request
            .identity
            .as_ref()
            .ok_or_else(|| PrCode::StaleContext.failure())?;
        let force = matches!(request.request, PrRequest::Refresh { .. });
        let observed = self
            .demand(
                request,
                scope.clone(),
                overview_read(route, identity.number),
                force,
            )
            .await?;
        let normalized = discussion::normalize_overview(&observed.body, identity, 0)?;
        let mut overview = normalized.overview;
        let mut grants = normalized.grants;
        let mut sections = Sections {
            commits: SectionState::NotLoaded,
            timeline: SectionState::NotLoaded,
            threads: SectionState::NotLoaded,
            thread_comments: SectionState::NotLoaded,
            reviewers: SectionState::NotLoaded,
            labels: SectionState::NotLoaded,
        };
        let mut availability = None;
        for collection in [CollectionKind::Reviewers, CollectionKind::Labels] {
            let authority = PageAuthority {
                identity: identity.clone(),
                version: normalized.version.clone(),
                collection,
                thread_id: None,
                thread_provider_id: None,
            };
            let result = self
                .demand(
                    request,
                    scope.clone(),
                    connection_read(route, identity.number, collection, None, None)?,
                    force,
                )
                .await;
            let result =
                result.and_then(|page| discussion::normalize_page(&page.body, &authority, None, 0));
            match result {
                Ok(page) => {
                    let Collection {
                        items,
                        total_count,
                        next_cursor,
                        completeness,
                        limit_reason,
                        observed_revision,
                    } = page.collection;
                    match collection {
                        CollectionKind::Reviewers => {
                            overview.reviewers = Collection {
                                items: items
                                    .into_iter()
                                    .map(|i| match i {
                                        Item::Reviewer { reviewer } => Ok(reviewer),
                                        _ => Err(PrCode::InvalidOutput.failure()),
                                    })
                                    .collect::<Result<_, _>>()?,
                                total_count,
                                next_cursor,
                                completeness,
                                limit_reason,
                                observed_revision,
                            };
                            sections.reviewers = SectionState::Available;
                        }
                        CollectionKind::Labels => {
                            overview.labels = Collection {
                                items: items
                                    .into_iter()
                                    .map(|i| match i {
                                        Item::Label { label } => Ok(label),
                                        _ => Err(PrCode::InvalidOutput.failure()),
                                    })
                                    .collect::<Result<_, _>>()?,
                                total_count,
                                next_cursor,
                                completeness,
                                limit_reason,
                                observed_revision,
                            };
                            sections.labels = SectionState::Available;
                        }
                        _ => unreachable!(),
                    }
                    grants.extend(page.grants);
                }
                Err(failure) => {
                    if matches!(
                        failure.code,
                        PrCode::ChangedSnapshot
                            | PrCode::StaleContext
                            | PrCode::AuthRequired
                            | PrCode::AuthUnavailable
                            | PrCode::AccessDenied
                            | PrCode::RepositoryUnavailable
                    ) {
                        return Err(failure);
                    }
                    if collection == CollectionKind::Reviewers {
                        sections.reviewers = SectionState::Unavailable;
                    } else {
                        sections.labels = SectionState::Unavailable;
                    }
                    availability.get_or_insert(failure);
                }
            }
        }
        Ok(Publication {
            result: PrSuccess::Snapshot {
                snapshot: Snapshot {
                    session_id: String::new(),
                    revision: 0,
                    pr_id: String::new(),
                    overview,
                    observed_at: observed.observed_at,
                    freshness: Freshness::Fresh,
                    availability,
                    sections,
                },
            }
            .into(),
            grants,
        })
    }

    async fn demand(
        &self,
        request: &ProviderRequest,
        scope: DemandScope,
        read: GhRead,
        force: bool,
    ) -> Result<Arc<CachedPage>, Failure> {
        let account = self
            .coordinator
            .account()
            .filter(|a| a.account_epoch == request.context.account_epoch)
            .ok_or_else(|| PrCode::StaleContext.failure())?;
        let mut lease = self.coordinator.request(scope, read)?;
        if force {
            self.coordinator.refresh(lease.id)?;
        }
        loop {
            let state = lease.snapshot();
            if !state.loading {
                if let Some(failure) = state.failure {
                    return Err(failure);
                }
                let notices = self.coordinator.account_changes();
                let notice = notices.borrow();
                if notice.account.as_ref() != Some(&account) {
                    return Err(PrCode::StaleContext.failure());
                }
                if !notice.authorization_known {
                    return Err(PrCode::AuthUnavailable.failure());
                }
                if state.retention_limited {
                    return Err(PrCode::ResourceLimit.failure());
                }
                if state.stale {
                    return Err(PrCode::Network.failure());
                }
                return state.page.ok_or_else(|| PrCode::InvalidOutput.failure());
            }
            lease.changed().await;
        }
    }
}
fn overview_read(route: &GithubRepository, number: u64) -> GhRead {
    GhRead::ReadOverview {
        owner: route.owner.clone(),
        repository: route.name.clone(),
        number,
    }
}
fn connection_read(
    route: &GithubRepository,
    number: u64,
    collection: CollectionKind,
    thread: Option<String>,
    cursor: Option<String>,
) -> Result<GhRead, Failure> {
    let connection = match collection {
        CollectionKind::Commits => Connection::Commits,
        CollectionKind::Timeline => Connection::Timeline,
        CollectionKind::Threads => Connection::Threads,
        CollectionKind::ThreadComments => Connection::ThreadComments {
            thread_id: thread.ok_or_else(|| PrCode::StaleCursor.failure())?,
        },
        CollectionKind::Reviewers => Connection::Reviewers,
        CollectionKind::Labels => Connection::Labels,
    };
    Ok(GhRead::ReadConnection {
        owner: route.owner.clone(),
        repository: route.name.clone(),
        number,
        connection,
        cursor,
    })
}
fn version(snapshot: &crate::github::service::SnapshotEvidence) -> PrVersion {
    PrVersion {
        base_oid: snapshot.base_oid.clone(),
        head_oid: snapshot.head_oid.clone(),
        lifecycle: snapshot.lifecycle,
        updated_at: snapshot.updated_at.clone(),
    }
}
