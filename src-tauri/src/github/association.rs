//! Resolves configured Git heads to verified hosted PRs without changing local repository state.
//! Session choices and renderer authority remain owned by the service.
mod hosted;
mod local;

use super::{
    coordinator::PrDemandCoordinator,
    model::{Failure, Freshness, GithubRepository, Lifecycle},
    service::HostAccount,
    PrIdentity,
};
use crate::{git::process::GitProcess, workspace::SelectedContext};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RepositoryName {
    pub owner: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadMappingInput {
    pub owner: String,
    pub repository: String,
    pub head_ref: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalHead {
    pub repository: RepositoryName,
    pub head_ref: String,
}
/// Native evidence, reusable only after revalidation against the admitted selected context.
#[derive(Clone, Debug)]
pub(crate) struct LocalCapture {
    pub common_storage: PathBuf,
    pub config_fingerprint: String,
    pub branch_label: Option<String>,
    pub heads: Vec<LocalHead>,
    pub repositories: Vec<RepositoryName>,
    pub unresolved: bool,
    requested_branch: Option<String>,
    evidence: Vec<u8>,
}
#[derive(Clone, Debug)]
pub(crate) struct VerifiedHead {
    pub repository: GithubRepository,
    pub head_ref: String,
}
#[derive(Clone, Debug)]
pub(crate) struct VerifiedCandidate {
    pub identity: PrIdentity,
    pub base_repository: GithubRepository,
    pub base_ref: String,
    pub head_repository: Option<GithubRepository>,
    pub head_ref: Option<String>,
    pub lifecycle: Lifecycle,
    pub title: String,
}
/// Explicitly known identity; callers retain this in account/config-scoped native resources.
#[derive(Clone, Debug)]
pub(crate) struct KnownPull {
    pub identity: PrIdentity,
    pub base_repository: GithubRepository,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolutionState {
    Single,
    None,
    Ambiguous,
    Unresolved,
    Unavailable,
}
#[derive(Clone, Debug)]
pub(crate) struct AssociationObservation {
    pub capture: LocalCapture,
    pub bases: Vec<GithubRepository>,
    pub heads: Vec<VerifiedHead>,
    pub candidates: Vec<VerifiedCandidate>,
    pub historical: Vec<VerifiedCandidate>,
    pub state: ResolutionState,
    pub complete: bool,
    pub failure: Option<Failure>,
    pub observed_at: u64,
    pub freshness: Freshness,
}
/// The coordinator owns account probes/cache/retries; dropping resolution releases its live lease.
pub(crate) struct AssociationResolver {
    git: GitProcess,
    coordinator: Arc<PrDemandCoordinator>,
    fingerprints: Mutex<Vec<(Vec<u8>, String)>>,
}
impl AssociationResolver {
    pub(crate) fn new(coordinator: Arc<PrDemandCoordinator>) -> Self {
        Self {
            git: GitProcess::default(),
            coordinator,
            fingerprints: Mutex::new(Vec::new()),
        }
    }
    pub(crate) async fn capture(
        &self,
        context: &SelectedContext,
        branch: Option<&str>,
    ) -> Result<LocalCapture, Failure> {
        self.capture_local(context, branch).await
    }
    pub(crate) async fn validate_capture(
        &self,
        context: &SelectedContext,
        capture: &LocalCapture,
    ) -> Result<(), Failure> {
        let current = self
            .capture(context, capture.requested_branch.as_deref())
            .await?;
        if current.evidence != capture.evidence {
            return Err(super::model::PrCode::StaleContext.failure());
        }
        Ok(())
    }
    pub(crate) async fn resolve(
        &self,
        context: &SelectedContext,
        capture: LocalCapture,
        account: &HostAccount,
        mapping: Option<&HeadMappingInput>,
        known: &[KnownPull],
    ) -> Result<AssociationObservation, Failure> {
        self.resolve_hosted(context, capture, account, mapping, known)
            .await
    }
}

#[cfg(test)]
#[path = "../../tests/integration/github_association.rs"]
mod integration_tests;
#[cfg(test)]
#[path = "../../tests/unit/github_association.rs"]
mod unit_tests;
