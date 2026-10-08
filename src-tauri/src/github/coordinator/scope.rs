//! Defines native cache grouping separately from worktree/session presentation authority.
use super::{Failure, GhRead, PrCode};
use crate::github::{
    model::{BaseKind, ComparisonSource},
    PrIdentity,
};
use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
};

#[derive(Clone)]
pub(crate) struct SnapshotVersion {
    pub revision: u64,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    pub source: Option<ComparisonSource>,
    pub base_kind: Option<BaseKind>,
}
impl PartialEq for SnapshotVersion {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.base_oid == other.base_oid
            && self.head_oid == other.head_oid
            && self.source.map(|s| std::mem::discriminant(&s))
                == other.source.map(|s| std::mem::discriminant(&s))
            && self.base_kind.map(|s| std::mem::discriminant(&s))
                == other.base_kind.map(|s| std::mem::discriminant(&s))
    }
}
impl Eq for SnapshotVersion {}
impl Hash for SnapshotVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.revision.hash(state);
        self.base_oid.hash(state);
        self.head_oid.hash(state);
        self.source.map(|s| std::mem::discriminant(&s)).hash(state);
        self.base_kind
            .map(|s| std::mem::discriminant(&s))
            .hash(state);
    }
}
/// Canonical common-storage path and remote fingerprint are native observations. Source endpoints
/// describe the captured immutable comparison; moving branch text alone cannot identify its bytes.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DemandScope {
    pub common_storage: PathBuf,
    pub config_fingerprint: String,
    pub mapped_branch: Option<String>,
    pub identity: Option<PrIdentity>,
    pub version: SnapshotVersion,
}
impl Hash for DemandScope {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.common_storage.hash(state);
        self.config_fingerprint.hash(state);
        self.mapped_branch.hash(state);
        self.version.hash(state);
        self.identity
            .as_ref()
            .map(|id| (&id.host, &id.base_repository_id, id.number))
            .hash(state);
    }
}
impl DemandScope {
    pub(super) fn validate(&self, read: &GhRead) -> Result<(), Failure> {
        let invalid = self.common_storage.as_os_str().len() > 4096
            || !self.common_storage.is_absolute()
            || self.config_fingerprint.len() > 256
            || self.mapped_branch.as_ref().is_some_and(|b| b.len() > 1024)
            || self
                .version
                .base_oid
                .as_ref()
                .is_some_and(|o| o.len() > 128)
            || self
                .version
                .head_oid
                .as_ref()
                .is_some_and(|o| o.len() > 128)
            || self.identity.as_ref().is_some_and(|id| {
                id.host != "github.com" || id.base_repository_id.len() > 256 || id.number == 0
            });
        if invalid {
            return Err(PrCode::InvalidOutput.failure());
        }
        let (owner, repo, extra) = match read {
            GhRead::ReadRepository { owner, repository }
            | GhRead::ReadOverview { owner, repository, .. }
            | GhRead::ReadPull {
                owner, repository, ..
            }
            | GhRead::ReadPullFiles {
                owner, repository, ..
            } => (owner, repository, 0),
            GhRead::ListPulls {
                owner,
                repository,
                head,
                ..
            } => (owner, repository, head.as_ref().map_or(0, String::len)),
            GhRead::ReadCommit {
                owner,
                repository,
                oid,
                ..
            } => (owner, repository, oid.len()),
            GhRead::ReadConnection {
                owner,
                repository,
                cursor,
                connection,
                ..
            } => (
                owner,
                repository,
                cursor.as_ref().map_or(0, String::len)
                    + match connection {
                        crate::github::transport::Connection::ThreadComments { thread_id } => {
                            thread_id.len()
                        }
                        _ => 0,
                    },
            ),
            _ => return Err(PrCode::InvalidOutput.failure()),
        };
        if owner.len() > 100 || repo.len() > 100 || extra > 8192 {
            return Err(PrCode::ResourceLimit.failure());
        }
        Ok(())
    }
}
pub(crate) enum Invalidation {
    Account,
    Repository(PathBuf),
    Configuration {
        common_storage: PathBuf,
        keep_fingerprint: String,
    },
    Branch {
        common_storage: PathBuf,
        branch: Option<String>,
    },
    Snapshot {
        identity: PrIdentity,
        keep: SnapshotVersion,
    },
}
impl Invalidation {
    pub(super) fn matches(&self, scope: &DemandScope) -> bool {
        match self {
            Self::Account => true,
            Self::Repository(common) => *common == scope.common_storage,
            Self::Configuration {
                common_storage,
                keep_fingerprint,
            } => {
                *common_storage == scope.common_storage
                    && *keep_fingerprint != scope.config_fingerprint
            }
            Self::Branch {
                common_storage,
                branch,
            } => *common_storage == scope.common_storage && *branch == scope.mapped_branch,
            Self::Snapshot { identity, keep } => {
                scope.identity.as_ref() == Some(identity) && scope.version != *keep
            }
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub scope: DemandScope,
    pub read: GhRead,
    pub user: String,
    pub epoch: u64,
}
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct CollectionKey(Key);
impl Key {
    pub fn collection(&self) -> CollectionKey {
        let mut key = self.clone();
        match &mut key.read {
            GhRead::ReadConnection { cursor, .. } => *cursor = None,
            GhRead::ReadPullFiles { page, .. }
            | GhRead::ListPulls { page, .. }
            | GhRead::ReadCommit { page, .. } => *page = 1,
            _ => {}
        }
        CollectionKey(key)
    }
}
