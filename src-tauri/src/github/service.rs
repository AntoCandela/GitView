//! Owns ephemeral PR authority and validates native context on both sides of provider I/O.
//!
//! Providers normalize data and verify upstream identities. This service issues read authority;
//! it never executes renderer strings or holds workspace/registry locks across provider awaits.
use std::{collections::{HashMap, VecDeque}, future::Future, pin::Pin, sync::Arc, time::Duration};
use parking_lot::Mutex;
use tokio::sync::watch;
use uuid::Uuid;
use super::{model::*, PrAuthority, PrContext, PrIdentity, PrSession};
use crate::{application::RepositoryService, workspace::SelectedContext};

const MAX_HANDLES: usize = 512;
const MAX_SESSIONS: usize = 32;
const MAX_PUBLICATION_BYTES: usize = 4 * 1024 * 1024;

/// Native account observation; no credential material is retained or sent to the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostAccount { pub host: GithubHost, pub provider_user_id: String, pub account_epoch: u64 }

/// Native-only resource targets. Providers verify their meaning before issuing a grant.
#[derive(Clone, Debug)]
pub enum Resource {
    Association,
    Candidate { association_id: String, identity: PrIdentity },
    Pr { identity: PrIdentity },
    Comparison,
    File { comparison_id: String, key: String },
    Cursor { comparison_id: Option<String>, collection: Option<CollectionKind>, thread_id: Option<String>, key: String },
    Thread { key: String }, Commit { key: String, parent_count: u32 }, Anchor { key: String }, Link { url: String },
}

/// A grant can only be created by native provider code; publication binds it to the request scope.
#[derive(Clone, Debug)]
pub struct Grant { id: String, pub resource: Resource }
impl Grant {
    pub fn new(resource: Resource) -> Self { Self { id: Uuid::new_v4().to_string(), resource } }
    pub fn id(&self) -> &str { &self.id }
}

/// Version evidence captured from the published snapshot, not reconstructed from moving refs.
#[derive(Clone, Debug)]
pub struct SnapshotEvidence {
    pub base_oid: Option<String>, pub head_oid: Option<String>, pub lifecycle: Lifecycle,
    pub updated_at: String, pub observed_at: u64,
}
impl SnapshotEvidence {
    fn capture(snapshot: &Snapshot) -> Self {
        Self { base_oid: snapshot.overview.base_oid.clone(), head_oid: snapshot.overview.head_oid.clone(),
            lifecycle: snapshot.overview.lifecycle, updated_at: snapshot.overview.updated_at.clone(), observed_at: snapshot.observed_at }
    }
}
#[derive(Clone, Debug)]
pub struct PublishedReview { pub session: PrSession, pub snapshot: SnapshotEvidence }
/// Native comparison authority retains the owning snapshot and exact source-specific endpoints.
#[derive(Clone, Debug)]
pub struct PublishedComparison {
    pub comparison_id: String, pub identity: PrIdentity, pub review: PublishedReview,
    pub source: ComparisonSource, pub scope: ComparisonScope, pub base: ComparisonBase,
    pub head_oid: Option<String>, pub full_content: bool,
    pub observed_at: u64, pub observed_head_oid: Option<String>, pub observed_base_oid: Option<String>, pub parent_oid: Option<String>,
}

pub struct Publication { pub result: PrResult, pub grants: Vec<Grant> }
impl From<PrCode> for Publication {
    fn from(code: PrCode) -> Self { Self { result: code.into(), grants: vec![] } }
}

/// Captured native repository and resolved resources, never a process-wide working directory.
/// Providers must use fixed operations and honor cancellation by dropping/reaping owned work.
pub struct ProviderRequest {
    pub repository: SelectedContext, pub context: PrContext, pub request: PrRequest, pub resources: Vec<Grant>,
    pub identity: Option<PrIdentity>, pub review: Option<PublishedReview>, pub comparison: Option<PublishedComparison>,
}

/// Account reads are in-memory observations. Authentication/network discovery belongs to bounded provider work.
pub trait PullRequestProvider: Send + Sync {
    fn account(&self) -> Option<HostAccount>;
    fn read(&self, request: ProviderRequest) -> Pin<Box<dyn Future<Output = Publication> + Send + '_>>;
}
struct UnconfiguredProvider;
impl PullRequestProvider for UnconfiguredProvider {
    fn account(&self) -> Option<HostAccount> { None }
    fn read(&self, _: ProviderRequest) -> Pin<Box<dyn Future<Output = Publication> + Send + '_>> {
        Box::pin(async { PrCode::IntegrationUnavailable.into() })
    }
}
struct Handle { comparison: Option<PublishedComparison>, context: PrContext, session: Option<PrSession>, grant: Grant }
struct ContextLifetime { context: PrContext, cancel: watch::Sender<bool> }
struct Session { snapshot: SnapshotEvidence, comparison_cancel: watch::Sender<bool>, comparison_epoch: u64, context: PrContext, session: PrSession, pr_id: String, cancel: watch::Sender<bool> }
struct Registry {
    contexts: HashMap<String, ContextLifetime>, account: Option<HostAccount>, authority: PrAuthority, handles: HashMap<String, Handle>,
    sessions: HashMap<String, Session>, released: VecDeque<(PrContext, String)>, association_epoch: u64,
}
impl Default for Registry {
    fn default() -> Self {
        Self { contexts: HashMap::new(), account: None, authority: PrAuthority::new(MAX_SESSIONS), handles: HashMap::new(), sessions: HashMap::new(), released: VecDeque::new(), association_epoch: 0 }
    }
}

pub struct PullRequestService { provider: Arc<dyn PullRequestProvider>, registry: Mutex<Registry> }
impl Default for PullRequestService {
    fn default() -> Self { Self::new(Arc::new(UnconfiguredProvider)) }
}
struct Ticket {
    context: PrContext, repository: SelectedContext, request: PrRequest, resources: Vec<Grant>,
    identity: Option<PrIdentity>, review: Option<PublishedReview>, comparison: Option<PublishedComparison>,
    session: Option<PrSession>, comparison_epoch: Option<u64>, account: Option<HostAccount>, association_epoch: Option<u64>, cancel: Option<watch::Receiver<bool>>, context_cancel: watch::Receiver<bool>, comparison_cancel: Option<watch::Receiver<bool>>,
}
enum Admission { Immediate(PrResult), Read(Ticket) }

impl PullRequestService {
    pub fn new(provider: Arc<dyn PullRequestProvider>) -> Self { Self { provider, registry: Mutex::new(Registry::default()) } }

    /// Invalidates private authority when a native account observation changes.
    /// Providers notify this boundary when authentication discovery changes their effective account.
    pub fn reconcile_account(&self) {
        let account = self.provider.account();
        let mut registry = self.registry.lock();
        registry.reconcile_account(account);
    }

    /// Revokes an entry's authority and wakes its pending session requests.
    pub fn invalidate_entry(&self, entry_id: &str) { self.registry.lock().invalidate_entry(entry_id); }

    /// Rejects forged resource handles before dispatch. Context must be captured natively.
    pub fn admit(&self, context: &PrContext, request: &PrRequest) -> Result<(), PrCode> {
        self.registry.lock().resolve(context, request).map(|_| ())
    }

    pub async fn execute(&self, repository: &RepositoryService, entry_id: &str, request: PrRequest) -> PrResult {
        let admission = repository.with_pull_request_context(entry_id, |native| {
            let Some((native, generation)) = native else { return Err(PrCode::StaleContext); };
            self.begin(native, generation, request)
        }).await;
        let ticket = match admission {
            Ok(Admission::Immediate(result)) => return result,
            Ok(Admission::Read(ticket)) => ticket,
            Err(code) => return code.into(),
        };
        let provider_request = ProviderRequest { repository: ticket.repository.clone(), context: ticket.context.clone(), request: ticket.request.clone(), resources: ticket.resources.clone(), identity: ticket.identity.clone(), review: ticket.review.clone(), comparison: ticket.comparison.clone() };
        let mut cancellation = ticket.cancel.clone();
        let mut context_cancel = ticket.context_cancel.clone();
        let mut comparison_cancel = ticket.comparison_cancel.clone();
        let cancelled = async {
            if let Some(receiver) = cancellation.as_mut() { let _ = receiver.changed().await; }
            else { std::future::pending::<()>().await; }
        };
        let comparison_cancelled = async {
            if let Some(receiver) = comparison_cancel.as_mut() { let _ = receiver.changed().await; }
            else { std::future::pending::<()>().await; }
        };
        let publication = tokio::select! {
            biased;
            _ = context_cancel.changed() => return PrCode::StaleContext.into(),
            _ = comparison_cancelled => return PrCode::StaleContext.into(),
            _ = cancelled => return PrCode::StaleContext.into(),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                if let PrRequest::Associations { branch: Some(branch) } = &ticket.request {
                    if branch.is_empty() || branch.len() > 1024 { return PrCode::UnresolvedMapping.into(); }
                    if let Err(code) = repository.validate_pull_request_branch(&ticket.repository, branch).await { return code.into(); }
                }
                let current = repository.with_pull_request_context(entry_id, |current| {
                    current.is_some_and(|(current, generation)| current.root == ticket.repository.root
                        && current.git_dir == ticket.repository.git_dir && current.identity == ticket.repository.identity
                        && generation == ticket.context.repository_generation)
                }).await;
                if !current || self.provider.account() != ticket.account { return PrCode::StaleContext.into(); }
                self.provider.read(provider_request).await
            }) => match result {
                Ok(result) => result,
                Err(_) => PrCode::Timeout.into(),
            },
        };
        repository.with_pull_request_context(entry_id, |current| {
            let Some((current, generation)) = current else { return PrCode::StaleContext.into(); };
            if current.root != ticket.repository.root || current.git_dir != ticket.repository.git_dir
                || current.identity != ticket.repository.identity || generation != ticket.context.repository_generation {
                self.invalidate_entry(entry_id);
                return PrCode::StaleContext.into();
            }
            self.publish(ticket, publication)
        }).await
    }

    fn begin(&self, repository: SelectedContext, generation: u64, request: PrRequest) -> Result<Admission, PrCode> {
        let account = self.provider.account();
        let context = PrContext { entry_id: repository.entry_id.clone(), repository_generation: generation, account_epoch: account.as_ref().map_or(0, |a| a.account_epoch) };
        let mut registry = self.registry.lock();
        registry.reconcile_account(account.clone());
        if registry.contexts.get(&context.entry_id).is_some_and(|current| current.context != context) {
            registry.invalidate_entry(&context.entry_id);
        }
        if !registry.contexts.contains_key(&context.entry_id) {
            if registry.contexts.len() >= MAX_SESSIONS { return Err(PrCode::ResourceLimit); }
            registry.contexts.insert(context.entry_id.clone(), ContextLifetime { context: context.clone(), cancel: watch::channel(false).0 });
        }
        let context_cancel = registry.contexts.get(&context.entry_id).ok_or(PrCode::StaleContext)?.cancel.subscribe();
        if matches!(request, PrRequest::Open { .. }) && registry.sessions.len() >= MAX_SESSIONS { return Err(PrCode::ResourceLimit); }
        if let PrRequest::Release { session_id } = &request {
            if registry.released.iter().any(|(owner, id)| owner == &context && id == session_id) {
                return Ok(Admission::Immediate(PrSuccess::Released.into()));
            }
        }
        let resources = registry.resolve(&context, &request)?;
        if let PrRequest::Choose { candidate_id, .. } = &request {
            let Resource::Candidate { identity, .. } = &registry.handle(&context, candidate_id)?.grant.resource else { return Err(PrCode::StaleContext); };
            let grant = Grant::new(Resource::Pr { identity: identity.clone() });
            let id = grant.id.clone();
            if registry.handles.len() >= MAX_HANDLES { return Err(PrCode::ResourceLimit); }
            registry.handles.insert(id.clone(), Handle { comparison: None, context, session: None, grant });
            return Ok(Admission::Immediate(PrSuccess::Chosen { pr_id: id }.into()));
        }
        let session_id = registry.request_session(&context, &request)?;
        let session = session_id.as_ref().and_then(|id| registry.sessions.get(id)).map(|s| s.session.clone());
        let review = session_id.as_ref().and_then(|id| registry.sessions.get(id)).map(|s| PublishedReview { session: s.session.clone(), snapshot: s.snapshot.clone() });
        let identity = if let Some(session) = &session {
            Some(registry.authority.validate(&context, session).map_err(|_| PrCode::StaleContext)?.clone())
        } else if let PrRequest::Open { pr_id } = &request {
            match &registry.handle(&context, pr_id)?.grant.resource { Resource::Pr { identity } => Some(identity.clone()), _ => return Err(PrCode::StaleContext) }
        } else { None };
        let comparison = match &request {
            PrRequest::File { comparison_id, .. } | PrRequest::FilesPage { comparison_id, .. } => {
                Some(registry.handle(&context, comparison_id)?.comparison.clone().ok_or(PrCode::StaleContext)?)
            }, _ => None,
        };
        let comparison_epoch = if matches!(request, PrRequest::Compare { .. } | PrRequest::ResolveAnchor { .. }) {
            let id = session_id.as_ref().ok_or(PrCode::StaleContext)?;
            let current = registry.sessions.get_mut(id).ok_or(PrCode::StaleContext)?;
            current.comparison_epoch = current.comparison_epoch.checked_add(1).ok_or(PrCode::ResourceLimit)?;
            current.comparison_cancel = watch::channel(false).0;
            let epoch = current.comparison_epoch;
            registry.handles.retain(|_, handle| handle.session.as_ref().is_none_or(|session| &session.id.to_string() != id)
                || !matches!(handle.grant.resource, Resource::Comparison | Resource::File { .. } | Resource::Cursor { comparison_id: Some(_), .. }));
            Some(epoch)
        } else { None };
        let comparison_cancel = if matches!(request, PrRequest::Compare { .. } | PrRequest::ResolveAnchor { .. } | PrRequest::File { .. } | PrRequest::FilesPage { .. }) {
            session_id.as_ref().and_then(|id| registry.sessions.get(id)).map(|s| s.comparison_cancel.subscribe())
        } else { None };
        let cancel = session_id.as_ref().and_then(|id| registry.sessions.get(id)).map(|s| s.cancel.subscribe());
        if let PrRequest::Release { session_id } = &request {
            registry.release(&context, session_id)?;
            return Ok(Admission::Immediate(PrSuccess::Released.into()));
        }
        let association_epoch = if matches!(request, PrRequest::Associations { .. } | PrRequest::MapHead { .. }) {
            registry.association_epoch = registry.association_epoch.checked_add(1).ok_or(PrCode::ResourceLimit)?;
            if matches!(request, PrRequest::Associations { .. }) {
                registry.handles.retain(|_, handle| handle.context != context || !matches!(handle.grant.resource, Resource::Association | Resource::Candidate { .. }));
            }
            Some(registry.association_epoch)
        } else { None };
        Ok(Admission::Read(Ticket { context, repository, request, resources, identity, review, comparison, session, comparison_epoch, account, association_epoch, cancel, context_cancel, comparison_cancel }))
    }

    fn publish(&self, ticket: Ticket, mut publication: Publication) -> PrResult {
        let account = self.provider.account();
        let mut registry = self.registry.lock();
        registry.reconcile_account(account.clone());
        if account != ticket.account { return PrCode::StaleContext.into(); }
        if ticket.association_epoch.is_some_and(|epoch| epoch != registry.association_epoch) { return PrCode::StaleContext.into(); }
        if let (Some(epoch), Some(session)) = (ticket.comparison_epoch, &ticket.session) {
            if registry.sessions.get(&session.id.to_string()).is_none_or(|current| current.comparison_epoch != epoch) { return PrCode::StaleContext.into(); }
        }
        if let Some(session) = &ticket.session {
            if registry.authority.validate(&ticket.context, session).is_err() { return PrCode::ChangedSnapshot.into(); }
        }
        if registry.resolve(&ticket.context, &ticket.request).is_err() { return PrCode::StaleContext.into(); }
        if let PrResult::Failure(_) = publication.result { return publication.result; }
        if let PrResult::Success(PrSuccess::Snapshot { snapshot }) = &publication.result {
            if ticket.identity.as_ref().is_none_or(|identity| identity.host != "github.com"
                || snapshot.overview.base_repository.host != GithubHost::GithubCom
                || identity.base_repository_id != snapshot.overview.base_repository.id || identity.number != snapshot.overview.number) {
                return PrCode::InvalidOutput.into();
            }
        }
        if !compatible(&ticket.request, &publication.result) { return PrCode::InvalidOutput.into(); }
        if publication.grants.len() + registry.handles.len() > MAX_HANDLES
            || publication.grants.iter().any(|grant| registry.handles.contains_key(&grant.id) || !valid_resource(&grant.resource))
            || serde_json::to_vec(&publication.result).map_or(true, |bytes| bytes.len() > MAX_PUBLICATION_BYTES) {
            return PrCode::ResourceLimit.into();
        }
        if !registry.valid_publication(&ticket, &publication) { return PrCode::InvalidOutput.into(); }
        let mut session = ticket.session.clone();
        if let PrResult::Success(PrSuccess::Snapshot { snapshot }) = &mut publication.result {
            match &ticket.request {
                PrRequest::Open { pr_id } => {
                    let Some(Handle { grant: Grant { resource: Resource::Pr { identity }, .. }, .. }) = registry.handles.get(pr_id) else { return PrCode::StaleContext.into(); };
                    let identity = identity.clone();
                    let Ok(issued) = registry.authority.open(ticket.context.clone(), identity) else { return PrCode::ResourceLimit.into(); };
                    let (cancel, _) = watch::channel(false);
                    snapshot.session_id = issued.id.to_string(); snapshot.pr_id = pr_id.clone(); snapshot.revision = issued.revision;
                    registry.sessions.insert(snapshot.session_id.clone(), Session { snapshot: SnapshotEvidence::capture(snapshot), comparison_cancel: watch::channel(false).0, comparison_epoch: 0, context: ticket.context.clone(), session: issued.clone(), pr_id: pr_id.clone(), cancel });
                    session = Some(issued);
                },
                PrRequest::Refresh { session_id } => {
                    let Some(previous) = ticket.session.as_ref() else { return PrCode::StaleContext.into(); };
                    let Ok(issued) = registry.authority.replace_snapshot(&ticket.context, previous) else { return PrCode::ChangedSnapshot.into(); };
                    registry.handles.retain(|_, h| h.session.as_ref().is_none_or(|s| s.id != issued.id));
                    let Some(current) = registry.sessions.get_mut(session_id) else { return PrCode::StaleContext.into(); };
                    current.session = issued.clone();
                    current.snapshot = SnapshotEvidence::capture(snapshot);
                    // Replacing the sender wakes old reads without cancelling future revision reads.
                    current.cancel = watch::channel(false).0;
                    snapshot.session_id = session_id.clone(); snapshot.pr_id = current.pr_id.clone(); snapshot.revision = issued.revision;
                    session = Some(issued);
                }, _ => return PrCode::InvalidOutput.into(),
            }
        }
        stamp_result(&mut publication.result, session.as_ref());
        let captured_comparison = match &publication.result {
            PrResult::Success(PrSuccess::Comparison { comparison } | PrSuccess::Resolved { comparison, .. }) => {
                match (ticket.identity.clone(), ticket.review.clone()) {
                    (Some(identity), Some(review)) => Some(PublishedComparison {
                        comparison_id: comparison.comparison_id.clone(), identity, review, source: comparison.source, scope: comparison.scope,
                        base: comparison.base.clone(), head_oid: comparison.head_oid.clone(), full_content: comparison.full_content,
                        observed_at: comparison.observed_at, observed_head_oid: comparison.observed_head_oid.clone(), observed_base_oid: comparison.observed_base_oid.clone(), parent_oid: comparison.parent_oid.clone(),
                    }), _ => None,
                }
            }, _ => None,
        };
        for grant in publication.grants {
            let comparison = captured_comparison.as_ref().filter(|comparison| comparison.comparison_id == grant.id).cloned();
            registry.handles.insert(grant.id.clone(), Handle { comparison, context: ticket.context.clone(), session: session.clone(), grant });
        }
        publication.result
    }
}

impl Registry {
    fn valid_publication(&self, ticket: &Ticket, publication: &Publication) -> bool {
        let mut ids = std::collections::HashSet::new();
        if !publication.grants.iter().all(|grant| ids.insert(grant.id.as_str())) { return false; }
        let lookup = |id: &str| {
            publication.grants.iter().find(|grant| grant.id == id).map(|grant| grant.resource.clone()).or_else(|| {
                if matches!(ticket.request, PrRequest::Open { .. } | PrRequest::Refresh { .. }) { return None; }
                self.handle(&ticket.context, id).ok().filter(|h| h.session == ticket.session).map(|h| h.grant.resource.clone())
            })
        };
        let mode_matches = match (&publication.result, ticket.comparison.as_ref()) {
            (PrResult::Success(PrSuccess::File { content, .. }), Some(comparison)) => match content {
                FileContent::Text { .. } => matches!(comparison.source, ComparisonSource::LocalGit) && comparison.full_content,
                FileContent::Patch { .. } => matches!(comparison.source, ComparisonSource::GithubPatch) && !comparison.full_content,
                FileContent::Unsupported { .. } => true,
            },
            (PrResult::Success(PrSuccess::Files { .. }), Some(comparison)) => matches!(comparison.source, ComparisonSource::GithubPatch) && !comparison.full_content,
            (PrResult::Success(PrSuccess::File { .. } | PrSuccess::Files { .. }), None) => false,
            _ => true,
        };
        mode_matches && super::publication::valid(&ticket.request, &publication.result, &lookup)
    }
    fn reconcile_account(&mut self, account: Option<HostAccount>) {
        if self.account != account {
            self.authority.invalidate_account(); self.contexts.clear(); self.handles.clear(); self.sessions.clear(); self.released.clear(); self.account = account;
        }
    }
    fn invalidate_entry(&mut self, entry: &str) {
        self.authority.invalidate_entry(entry);
        self.contexts.remove(entry);
        self.handles.retain(|_, h| h.context.entry_id != entry);
        self.sessions.retain(|_, s| s.context.entry_id != entry);
        self.released.retain(|(context, _)| context.entry_id != entry);
    }
    fn handle(&self, context: &PrContext, id: &str) -> Result<&Handle, PrCode> {
        let handle = self.handles.get(id).filter(|h| &h.context == context).ok_or(PrCode::StaleContext)?;
        if let Some(session) = &handle.session { self.authority.validate(context, session).map_err(|_| PrCode::ChangedSnapshot)?; }
        Ok(handle)
    }
    fn session(&self, context: &PrContext, id: &str) -> Result<&Session, PrCode> {
        let session = self.sessions.get(id).filter(|s| &s.context == context).ok_or(PrCode::StaleContext)?;
        self.authority.validate(context, &session.session).map_err(|_| PrCode::ChangedSnapshot)?;
        Ok(session)
    }
    fn request_session(&self, context: &PrContext, request: &PrRequest) -> Result<Option<String>, PrCode> {
        match request {
            PrRequest::Page { session_id, .. } | PrRequest::Refresh { session_id } | PrRequest::Compare { session_id, .. }
            | PrRequest::ResolveAnchor { session_id, .. } | PrRequest::Release { session_id } | PrRequest::OpenLink { session_id, .. } => {
                self.session(context, session_id)?; Ok(Some(session_id.clone()))
            },
            PrRequest::FilesPage { comparison_id, .. } | PrRequest::File { comparison_id, .. } => {
                self.handle(context, comparison_id)?.session.as_ref().map(|s| Some(s.id.to_string())).ok_or(PrCode::StaleContext)
            },
            _ => Ok(None),
        }
    }
    fn resolve(&self, context: &PrContext, request: &PrRequest) -> Result<Vec<Grant>, PrCode> {
        let session_id = self.request_session(context, request)?;
        let mut resolved = vec![];
        let mut resolve = |id: &str, predicate: &dyn Fn(&Resource) -> bool| -> Result<(), PrCode> {
            let handle = self.handle(context, id)?;
            if !predicate(&handle.grant.resource) || handle.session.as_ref().map(|s| s.id.to_string()) != session_id { return Err(PrCode::StaleContext); }
            resolved.push(handle.grant.clone()); Ok(())
        };
        match request {
            PrRequest::Status | PrRequest::Associations { .. } | PrRequest::Refresh { .. } | PrRequest::Release { .. } => {},
            PrRequest::MapHead { association_id, owner, repository, head_ref } => {
                if !mapping_name(owner, 39) || !mapping_name(repository, 100) || !valid_head_ref(head_ref) { return Err(PrCode::UnresolvedMapping); }
                resolve(association_id, &|r| matches!(r, Resource::Association))?;
            },
            PrRequest::Choose { association_id, candidate_id } => {
                resolve(association_id, &|r| matches!(r, Resource::Association))?;
                resolve(candidate_id, &|r| matches!(r, Resource::Candidate { association_id: id, .. } if id == association_id))?;
            },
            PrRequest::Open { pr_id } => resolve(pr_id, &|r| matches!(r, Resource::Pr { .. }))?,
            PrRequest::Page { collection, cursor, thread_id, .. } => {
                if (*collection == CollectionKind::ThreadComments) != thread_id.is_some() { return Err(PrCode::StaleCursor); }
                if let Some(id) = thread_id { resolve(id, &|r| matches!(r, Resource::Thread { .. }))?; }
                if let Some(id) = cursor {
                    resolve(id, &|r| matches!(r, Resource::Cursor { comparison_id: None, collection: Some(c), thread_id: thread, .. } if c == collection && thread == thread_id)).map_err(|_| PrCode::StaleCursor)?;
                }
            },
            PrRequest::Compare { selection, .. } => if let ComparisonSelection::Commit { commit_id, parent_index } = selection {
                resolve(commit_id, &|r| matches!(r, Resource::Commit { parent_count, .. } if parent_index.is_none_or(|i| i < *parent_count)))?;
            },
            PrRequest::FilesPage { comparison_id, cursor } => {
                resolve(comparison_id, &|r| matches!(r, Resource::Comparison))?;
                resolve(cursor, &|r| matches!(r, Resource::Cursor { comparison_id: Some(id), collection: None, thread_id: None, .. } if id == comparison_id)).map_err(|_| PrCode::StaleCursor)?;
            },
            PrRequest::ResolveAnchor { anchor_id, .. } => resolve(anchor_id, &|r| matches!(r, Resource::Anchor { .. }))?,
            PrRequest::File { comparison_id, file_id } => {
                resolve(comparison_id, &|r| matches!(r, Resource::Comparison))?;
                resolve(file_id, &|r| matches!(r, Resource::File { comparison_id: id, .. } if id == comparison_id))?;
            },
            PrRequest::OpenLink { link_id, .. } => resolve(link_id, &|r| matches!(r, Resource::Link { url } if valid_link(url)))?,
        }
        Ok(resolved)
    }
    fn release(&mut self, context: &PrContext, id: &str) -> Result<(), PrCode> {
        let session = self.session(context, id)?.session.clone();
        self.authority.release(context, session.id).map_err(|_| PrCode::StaleContext)?;
        self.sessions.remove(id);
        self.handles.retain(|_, handle| handle.session.as_ref().is_none_or(|s| s.id != session.id));
        if self.released.len() == MAX_SESSIONS { self.released.pop_front(); }
        self.released.push_back((context.clone(), id.to_owned()));
        Ok(())
    }
}
fn mapping_name(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && value != "." && value != ".."
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}
fn valid_head_ref(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.starts_with(['/', '-']) && !value.ends_with(['/', '.'])
        && !value.contains([ '\0', '\\', ' ', '~', '^', ':', '?', '*', '[' ]) && !value.contains("..") && !value.contains("@{")
        && !value.bytes().any(|c| c.is_ascii_control())
}
fn valid_link(value: &str) -> bool {
    value.len() <= 4096 && url::Url::parse(value).is_ok_and(|url| url.scheme() == "https" && url.host_str() == Some("github.com")
        && url.username().is_empty() && url.password().is_none() && url.port().is_none())
}
fn valid_resource(resource: &Resource) -> bool {
    match resource {
        Resource::File { key, .. } | Resource::Cursor { key, .. } | Resource::Thread { key } | Resource::Commit { key, .. } | Resource::Anchor { key } => key.len() <= 4096,
        Resource::Candidate { identity, .. } | Resource::Pr { identity } => identity.host == "github.com" && !identity.base_repository_id.is_empty() && identity.base_repository_id.len() <= 256 && identity.number > 0,
        Resource::Link { url } => valid_link(url),
        _ => true,
    }
}
fn compatible(request: &PrRequest, result: &PrResult) -> bool {
    matches!((request, result),
        (PrRequest::Status, PrResult::Success(PrSuccess::Ready))
        | (PrRequest::Associations { .. } | PrRequest::MapHead { .. }, PrResult::Success(PrSuccess::Association { .. }))
        | (PrRequest::Open { .. } | PrRequest::Refresh { .. }, PrResult::Success(PrSuccess::Snapshot { .. }))
        | (PrRequest::Page { .. }, PrResult::Success(PrSuccess::Page { .. }))
        | (PrRequest::Compare { .. }, PrResult::Success(PrSuccess::Comparison { .. }))
        | (PrRequest::FilesPage { .. }, PrResult::Success(PrSuccess::Files { .. }))
        | (PrRequest::ResolveAnchor { .. }, PrResult::Success(PrSuccess::Resolved { .. } | PrSuccess::Fallback { .. }))
        | (PrRequest::File { .. }, PrResult::Success(PrSuccess::File { .. }))
        | (PrRequest::OpenLink { .. }, PrResult::Success(PrSuccess::Opened | PrSuccess::Blocked)))
}
fn stamp_result(result: &mut PrResult, session: Option<&PrSession>) {
    let Some(session) = session else { return; };
    match result {
        PrResult::Success(PrSuccess::Snapshot { snapshot }) => {
            snapshot.overview.reviewers.observed_revision = session.revision; snapshot.overview.labels.observed_revision = session.revision;
        },
        PrResult::Success(PrSuccess::Comparison { comparison } | PrSuccess::Resolved { comparison, .. }) => {
            comparison.session_id = session.id.to_string(); comparison.revision = session.revision; comparison.files.observed_revision = session.revision;
        },
        PrResult::Success(PrSuccess::Page { collection }) => collection.observed_revision = session.revision,
        PrResult::Success(PrSuccess::Files { collection }) => collection.observed_revision = session.revision,
        _ => {},
    }
}
#[cfg(test)]
#[path = "../../tests/unit/github_service.rs"]
mod unit_tests;
