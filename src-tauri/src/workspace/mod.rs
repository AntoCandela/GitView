//! Owns atomic workspace transitions and immutable renderer snapshots.
//!
//! Git probing belongs outside this lock. Request generations reject superseded
//! completions; snapshot revisions order externally visible state changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::Mutex;

use crate::git::{GitError, Head, RepositoryFacts, RepositoryKind};
use self::persistence::{PersistenceError, SavedRepository, WorkspaceDocument};

pub mod persistence;

/// Verified repository layout, or unknown until a restored location has been inspected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Unknown,
    WorkingTree,
    Bare,
}

impl From<RepositoryKind> for EntryKind {
    fn from(kind: RepositoryKind) -> Self {
        match kind {
            RepositoryKind::WorkingTree => Self::WorkingTree,
            RepositoryKind::Bare => Self::Bare,
        }
    }
}

/// Renderer-safe HEAD description, not a reference the renderer may resolve itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HeadLabel {
    Unknown,
    Branch {
        name: String,
    },
    Detached {
        #[serde(rename = "shortOid")]
        short_oid: String,
    },
    Unborn {
        name: String,
    },
}

impl From<Head> for HeadLabel {
    fn from(head: Head) -> Self {
        match head {
            Head::Branch(name) => Self::Branch { name },
            Head::Detached(short_oid) => Self::Detached { short_oid },
            Head::Unborn(name) => Self::Unborn { name },
        }
    }
}

/// Last probe lifecycle state, independent of staged or unstaged changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Checking,
    Unavailable,
}

/// Renderer-facing entry with an opaque ID and display-only repository/location labels.
///
/// Native identity and roots remain in the store; labels are never path inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryEntry {
    pub id: String,
    pub kind: EntryKind,
    pub repository_label: String,
    pub location_label: String,
    pub head: HeadLabel,
    pub availability: Availability,
}

/// Coherent, owned renderer view in admission order, with optional active selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    /// Increases only for visible state changes, not every issued probe generation.
    pub revision: u64,
    pub entries: Vec<RepositoryEntry>,
    pub active_context_id: Option<String>,
    pub restoring: bool,
    pub persistence_error: Option<PersistenceError>,
}

/// Admission result and current snapshot; opening or reusing never selects the entry.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenOutcome {
    Cancelled {
        snapshot: WorkspaceSnapshot,
    },
    Opened {
        #[serde(rename = "entryId")]
        entry_id: String,
        snapshot: WorkspaceSnapshot,
    },
    Reused {
        #[serde(rename = "entryId")]
        entry_id: String,
        snapshot: WorkspaceSnapshot,
    },
    Rejected {
        code: GitError,
        snapshot: WorkspaceSnapshot,
    },
}

/// Selection result; an unknown ID returns the unchanged snapshot.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelectOutcome {
    Selected { snapshot: WorkspaceSnapshot },
    NotFound { snapshot: WorkspaceSnapshot },
}

/// Stable app-state rejection facts, including admission failures propagated from Git probing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRejectionCode {
    InvalidDisplayName,
    RepositoryChanged,
    RepositoryUnavailable,
    SupersededSelection,
    GitUnavailable,
    NotRepository,
    Inaccessible,
    UnsafeRepository,
    ProbeTimeout,
    UnsupportedPathEncoding,
}

impl From<GitError> for WorkspaceRejectionCode {
    fn from(error: GitError) -> Self {
        match error {
            GitError::GitUnavailable => Self::GitUnavailable,
            GitError::NotRepository => Self::NotRepository,
            GitError::Inaccessible => Self::Inaccessible,
            GitError::UnsafeRepository => Self::UnsafeRepository,
            GitError::ProbeTimeout => Self::ProbeTimeout,
            GitError::RepositoryChanged => Self::RepositoryChanged,
            GitError::UnsupportedPathEncoding => Self::UnsupportedPathEncoding,
            GitError::Unavailable => Self::RepositoryUnavailable,
        }
    }
}

/// App-only label/removal or native worktree selection result; rejected edits leave choices unchanged.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationOutcome {
    Updated { snapshot: WorkspaceSnapshot },
    NotFound { snapshot: WorkspaceSnapshot },
    Rejected { code: WorkspaceRejectionCode, snapshot: WorkspaceSnapshot },
}

/// Stored filesystem identity prevents a different repository at the same canonical path from recovering.
///
/// Capturing these values is native I/O owned by the observation/application boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeIdentity {
    pub(crate) root: DirectoryIdentity,
    pub(crate) git_dir: DirectoryIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryIdentity {
    #[cfg(unix)]
    pub(crate) device: u64,
    #[cfg(unix)]
    pub(crate) inode: u64,
    #[cfg(not(unix))]
    pub(crate) created: std::time::SystemTime,
}

/// Native facts are absent until a restored location has passed its first verification.
#[derive(Clone)]
pub(crate) struct VerifiedIdentity {
    pub(crate) git_dir: PathBuf,
    pub(crate) identity: NativeIdentity,
}

struct StoredEntry {
    entry: RepositoryEntry,
    root: PathBuf,
    display_name: Option<String>,
    verified: Option<VerifiedIdentity>,
    // Latest request allowed to update this entry, even if it changed no visible fields.
    generation: u64,
}

struct WorkspaceState {
    revision: u64,
    id_prefix: String,
    next_id: u64,
    // One request sequence orders opens, refreshes and selections across entries.
    next_request: u64,
    entries: Vec<StoredEntry>,
    // Prevent an older in-flight reopen from resurrecting a deliberately removed choice.
    removed_roots: HashMap<PathBuf, u64>,
    active_context_id: Option<String>,
    restoring: bool,
    persistence_error: Option<PersistenceError>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);
        let started = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        Self {
            revision: 0,
            id_prefix: format!("{started:x}-{:x}-{:x}", std::process::id(), NEXT_SESSION.fetch_add(1, Ordering::Relaxed)),
            next_id: 0,
            next_request: 0,
            entries: Vec::new(),
            removed_roots: HashMap::new(),
            active_context_id: None,
            restoring: false,
            persistence_error: None,
        }
    }
}

impl WorkspaceState {
    fn next_entry_id(&mut self) -> String {
        self.next_id += 1;
        format!("repository-{}-{}", self.id_prefix, self.next_id)
    }

    fn next_generation(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }

    fn selected_entry(&self) -> Option<&StoredEntry> {
        let active = self.active_context_id.as_ref()?;
        self.entries.iter().find(|stored| &stored.entry.id == active)
    }

    fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            revision: self.revision,
            entries: self
                .entries
                .iter()
                .map(|stored| stored.entry.clone())
                .collect(),
            active_context_id: self.active_context_id.clone(),
            restoring: self.restoring,
            persistence_error: self.persistence_error.clone(),
        }
    }

    fn update_entry(&mut self, position: usize, facts: RepositoryFacts, identity: NativeIdentity) {
        let stored = &mut self.entries[position];
        if stored.verified.is_none() {
            stored.verified = Some(VerifiedIdentity { git_dir: facts.git_dir, identity });
        }
        let entry = &mut stored.entry;
        let updated = RepositoryEntry {
            id: entry.id.clone(),
            kind: facts.kind.into(),
            repository_label: stored.display_name.clone().unwrap_or(facts.repository_label),
            location_label: facts.location_label,
            head: facts.head.into(),
            availability: Availability::Available,
        };
        if *entry != updated {
            *entry = updated;
            self.revision += 1;
        }
    }

    fn mark_unavailable(&mut self, position: usize) {
        let entry = &mut self.entries[position].entry;
        if entry.availability != Availability::Unavailable {
            entry.availability = Availability::Unavailable;
            self.revision += 1;
        }
    }
}

/// Serializes in-memory transitions only; callers perform native I/O between tickets.
#[derive(Default)]
pub(crate) struct WorkspaceStore {
    state: Mutex<WorkspaceState>,
}

/// Open request generation reserved before probing; newer entry requests take precedence.
pub(crate) struct OpenTicket(u64);

/// Native probe target plus the entry generation required to commit its result.
pub(crate) struct RefreshTicket {
    pub(crate) root: PathBuf,
    pub(crate) verified: Option<VerifiedIdentity>,
    entry_id: String,
    generation: u64,
}

pub(crate) enum RefreshPublication {
    Verified,
    Unavailable(GitError),
    Superseded,
}

/// Immutable scan facts captured only from an admitted, currently selected native context.
pub(crate) struct SelectedContext {
    pub(crate) entry_id: String,
    pub(crate) root: PathBuf,
    pub(crate) git_dir: PathBuf,
    pub(crate) kind: RepositoryKind,
    pub(crate) identity: NativeIdentity,
}

impl WorkspaceStore {
    pub(crate) async fn snapshot(&self) -> WorkspaceSnapshot {
        self.state.lock().await.snapshot()
    }

    pub(crate) async fn active_context_id(&self) -> Option<String> {
        self.state.lock().await.active_context_id.clone()
    }

    /// Seeds choices before Git I/O; tickets are reserved now so later user intent wins.
    pub(crate) async fn restore(&self, document: WorkspaceDocument) -> Vec<RefreshTicket> {
        let mut state = self.state.lock().await;
        let mut tickets = Vec::with_capacity(document.repositories.len());
        for saved in document.repositories {
            let entry_id = state.next_entry_id();
            let generation = state.next_generation();
            let location_label = saved.root.to_str().expect("validated saved root").to_owned();
            let repository_label = saved.display_name.as_deref().unwrap_or_else(|| {
                saved.root.file_name().and_then(|name| name.to_str()).unwrap_or(&location_label)
            }).to_owned();
            if document.active_root.as_ref() == Some(&saved.root) {
                state.active_context_id = Some(entry_id.clone());
            }
            tickets.push(RefreshTicket {
                root: saved.root.clone(), verified: None, entry_id: entry_id.clone(), generation,
            });
            state.entries.push(StoredEntry {
                entry: RepositoryEntry {
                    id: entry_id, kind: EntryKind::Unknown, repository_label, location_label,
                    head: HeadLabel::Unknown, availability: Availability::Checking,
                },
                root: saved.root, display_name: saved.display_name, verified: None, generation,
            });
        }
        state.restoring = !tickets.is_empty();
        if state.restoring {
            state.revision += 1;
        }
        if let Some(active_id) = state.active_context_id.as_deref() {
            if let Some(position) = tickets.iter().position(|ticket| ticket.entry_id == active_id) {
                // Move only the probe ticket; admission order remains exactly as saved.
                let selected = tickets.remove(position);
                tickets.insert(0, selected);
            }
        }
        tickets
    }

    pub(crate) async fn finish_restoration(&self) {
        let mut state = self.state.lock().await;
        if state.restoring {
            state.restoring = false;
            state.revision += 1;
        }
    }

    pub(crate) async fn set_persistence_error(&self, error: Option<PersistenceError>) {
        let mut state = self.state.lock().await;
        if state.persistence_error != error {
            state.persistence_error = error;
            state.revision += 1;
        }
    }

    /// Captures only durable choices; the caller must release this lock before writing.
    pub(crate) async fn document(&self) -> WorkspaceDocument {
        let state = self.state.lock().await;
        WorkspaceDocument {
            version: 1,
            repositories: state.entries.iter().map(|stored| SavedRepository {
                root: stored.root.clone(), display_name: stored.display_name.clone(),
            }).collect(),
            active_root: state.selected_entry().map(|stored| stored.root.clone()),
        }
    }

    pub(crate) async fn selected_unverified_id(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.selected_entry().filter(|stored| stored.verified.is_none())
            .map(|stored| stored.entry.id.clone())
    }

    pub(crate) async fn selected_unavailable_unverified_id(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.selected_entry().filter(|stored| {
            stored.verified.is_none() && stored.entry.availability == Availability::Unavailable
        }).map(|stored| stored.entry.id.clone())
    }

    /// Recovery cannot recheck a formerly selected location or supersede a verified context.
    pub(crate) async fn begin_selected_recovery(&self, entry_id: &str) -> Option<RefreshTicket> {
        let mut state = self.state.lock().await;
        if state.active_context_id.as_deref() != Some(entry_id) {
            return None;
        }
        let position = state.entries.iter().position(|stored| stored.entry.id == entry_id && stored.verified.is_none())?;
        let generation = state.next_generation();
        let stored = &mut state.entries[position];
        stored.generation = generation;
        Some(RefreshTicket {
            root: stored.root.clone(), verified: None, entry_id: entry_id.to_owned(), generation,
        })
    }

    /// Reserves ordering without changing any renderer-visible state.
    pub(crate) async fn begin_open(&self) -> OpenTicket {
        OpenTicket(self.state.lock().await.next_generation())
    }

    /// Deduplicates canonical Git-directory identity and admits without selecting.
    ///
    /// Existing identity must retain its root and kind. An older ticket may reuse
    /// the ID, but cannot overwrite facts from a newer request.
    pub(crate) async fn admit(&self, ticket: OpenTicket, facts: RepositoryFacts, identity: NativeIdentity) -> OpenOutcome {
        let mut state = self.state.lock().await;
        if state.removed_roots.get(&facts.root).is_some_and(|generation| ticket.0 <= *generation) {
            return OpenOutcome::Cancelled { snapshot: state.snapshot() };
        }
        if let Some(position) = state.entries.iter().position(|stored| {
            stored.root == facts.root
                || stored.verified.as_ref().is_some_and(|verified| verified.git_dir == facts.git_dir)
        }) {
            let stored = &state.entries[position];
            if stored.root != facts.root
                || stored.verified.as_ref().is_some_and(|verified| {
                    verified.git_dir != facts.git_dir
                        || verified.identity != identity
                        || stored.entry.kind != EntryKind::from(facts.kind)
                })
            {
                return OpenOutcome::Rejected {
                    code: GitError::RepositoryChanged,
                    snapshot: state.snapshot(),
                };
            }
            let entry_id = stored.entry.id.clone();
            // Reopening must invalidate an older refresh without replacing the entry ID.
            if ticket.0 > state.entries[position].generation {
                state.entries[position].generation = ticket.0;
                state.update_entry(position, facts, identity);
            }
            return OpenOutcome::Reused { entry_id, snapshot: state.snapshot() };
        }
        state.removed_roots.remove(&facts.root);
        let entry_id = state.next_entry_id();
        state.entries.push(StoredEntry {
            entry: RepositoryEntry {
                id: entry_id.clone(),
                kind: facts.kind.into(),
                repository_label: facts.repository_label,
                location_label: facts.location_label,
                head: facts.head.into(),
                availability: Availability::Available,
            },
            root: facts.root,
            display_name: None,
            verified: Some(VerifiedIdentity { git_dir: facts.git_dir, identity }),
            generation: ticket.0,
        });
        state.revision += 1;
        OpenOutcome::Opened { entry_id, snapshot: state.snapshot() }
    }

    /// Renames only the app's display label; native roots and Git identity are untouched.
    pub(crate) async fn rename(&self, entry_id: &str, display_name: &str) -> MutationOutcome {
        let mut state = self.state.lock().await;
        let Some(position) = state.entries.iter().position(|stored| stored.entry.id == entry_id) else {
            return MutationOutcome::NotFound { snapshot: state.snapshot() };
        };
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return MutationOutcome::Rejected {
                code: WorkspaceRejectionCode::InvalidDisplayName, snapshot: state.snapshot(),
            };
        }
        let stored = &mut state.entries[position];
        if stored.display_name.as_deref() != Some(display_name) {
            stored.display_name = Some(display_name.to_owned());
            stored.entry.repository_label = display_name.to_owned();
            state.revision += 1;
        }
        MutationOutcome::Updated { snapshot: state.snapshot() }
    }

    /// Removes a choice, never its files; active removal picks the first other available entry.
    pub(crate) async fn remove(&self, entry_id: &str) -> MutationOutcome {
        let mut state = self.state.lock().await;
        let Some(position) = state.entries.iter().position(|stored| stored.entry.id == entry_id) else {
            return MutationOutcome::NotFound { snapshot: state.snapshot() };
        };
        let removed = state.entries.remove(position);
        let generation = state.next_generation();
        state.removed_roots.insert(removed.root, generation);
        if state.active_context_id.as_deref() == Some(entry_id) {
            let replacement = state.entries.iter().position(|stored| stored.entry.availability == Availability::Available);
            state.active_context_id = replacement.map(|position| {
                let stored = &mut state.entries[position];
                stored.generation = generation;
                stored.entry.availability = Availability::Checking;
                stored.entry.id.clone()
            });
        }
        state.revision += 1;
        MutationOutcome::Updated { snapshot: state.snapshot() }
    }

    /// Activates a known ID and invalidates pending updates even when reselected.
    ///
    /// Marks availability checking without I/O; unknown IDs leave state untouched.
    pub(crate) async fn select(&self, entry_id: &str) -> SelectOutcome {
        let mut state = self.state.lock().await;
        let Some(position) = state
            .entries
            .iter()
            .position(|stored| stored.entry.id == entry_id)
        else {
            return SelectOutcome::NotFound {
                snapshot: state.snapshot(),
            };
        };
        let generation = state.next_generation();
        state.entries[position].generation = generation;
        if state.entries[position].entry.availability != Availability::Checking {
            state.entries[position].entry.availability = Availability::Checking;
            state.revision += 1;
        }
        if state.active_context_id.as_deref() != Some(entry_id) {
            state.active_context_id = Some(entry_id.to_owned());
            state.revision += 1;
        }
        SelectOutcome::Selected {
            snapshot: state.snapshot(),
        }
    }

    /// Returns host facts only for the active ID; observation never admits or selects an arbitrary ID.
    pub(crate) async fn selected_context(&self, entry_id: &str) -> Option<SelectedContext> {
        let state = self.state.lock().await;
        if state.active_context_id.as_deref() != Some(entry_id) {
            return None;
        }
        let stored = state.selected_entry()?;
        let verified = stored.verified.as_ref()?;
        Some(SelectedContext {
            entry_id: stored.entry.id.clone(),
            root: stored.root.clone(),
            git_dir: verified.git_dir.clone(),
            kind: match stored.entry.kind {
                EntryKind::Unknown => return None,
                EntryKind::WorkingTree => RepositoryKind::WorkingTree,
                EntryKind::Bare => RepositoryKind::Bare,
            },
            identity: verified.identity.clone(),
        })
    }

    /// Captures the host-owned root and supersedes prior requests for this entry.
    ///
    /// The checking transition remains valid if its caller is cancelled.
    pub(crate) async fn begin_refresh(&self, entry_id: &str) -> Option<RefreshTicket> {
        let mut state = self.state.lock().await;
        let position = state
            .entries
            .iter()
            .position(|stored| stored.entry.id == entry_id)?;
        let generation = state.next_generation();
        let stored = &mut state.entries[position];
        stored.generation = generation;
        let ticket = RefreshTicket {
            root: stored.root.clone(),
            verified: stored.verified.clone(),
            entry_id: stored.entry.id.clone(),
            generation: stored.generation,
        };
        if stored.entry.availability != Availability::Checking {
            stored.entry.availability = Availability::Checking;
            state.revision += 1;
        }
        Some(ticket)
    }

    /// Applies a current ticket only when identity, root and kind still match.
    ///
    /// Probe failures or changed identities mark that entry unavailable while retaining
    /// its last labels and HEAD. Returns the publication outcome; superseded results change nothing.
    pub(crate) async fn complete_refresh(
        &self,
        ticket: RefreshTicket,
        result: Result<(RepositoryFacts, NativeIdentity), GitError>,
    ) -> RefreshPublication {
        let mut state = self.state.lock().await;
        if let Some(position) = state.entries.iter().position(|stored| {
            stored.entry.id == ticket.entry_id && stored.generation == ticket.generation
        }) {
            let stored = &state.entries[position];
            match result {
                Ok((facts, identity))
                    if facts.root == stored.root
                        && stored.verified.as_ref().is_none_or(|verified| {
                            facts.git_dir == verified.git_dir
                                && identity == verified.identity
                                && EntryKind::from(facts.kind) == stored.entry.kind
                        })
                        && !state.entries.iter().enumerate().any(|(other_position, other)| {
                            other_position != position
                                && other.verified.as_ref().is_some_and(|verified| verified.git_dir == facts.git_dir)
                        }) =>
                {
                    state.update_entry(position, facts, identity);
                    return RefreshPublication::Verified;
                }
                result => {
                    state.mark_unavailable(position);
                    return RefreshPublication::Unavailable(result.err().unwrap_or(GitError::RepositoryChanged));
                }
            }
        }
        RefreshPublication::Superseded
    }
}
