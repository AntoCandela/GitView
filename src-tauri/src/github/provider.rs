//! Composes verified branch association reads without enabling unfinished PR review operations.
//! The service owns grants and choices; this provider retains no second authority registry.
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::watch;

use super::{
    association::{
        AssociationObservation, AssociationResolver, HeadMappingInput, KnownPull, ResolutionState,
        VerifiedCandidate,
    },
    coordinator::{AccountNotice, PrDemandCoordinator},
    model::{
        Association, AssociationHead, AssociationState, Candidate, Failure, PrCode, PrRequest,
        PrResult, PrSuccess,
    },
    service::{
        AssociationBinding, Grant, HostAccount, ProviderRequest, Publication, PullRequestProvider,
        Resource,
    },
};
use crate::workspace::SelectedContext;

/// Opt-in native composition. Application defaults remain unconfigured until review reads exist.
pub(crate) struct AssociationProvider {
    coordinator: Arc<PrDemandCoordinator>,
    resolver: AssociationResolver,
}

impl AssociationProvider {
    pub(crate) fn new(coordinator: Arc<PrDemandCoordinator>) -> Self {
        Self {
            resolver: AssociationResolver::new(coordinator.clone()),
            coordinator,
        }
    }

    async fn association(&self, request: &ProviderRequest) -> Result<Publication, Failure> {
        let account = self
            .coordinator
            .account()
            .ok_or_else(|| PrCode::AuthUnavailable.failure())?;
        if account.account_epoch != request.context.account_epoch {
            return Err(PrCode::StaleContext.failure());
        }
        let previous = request
            .resources
            .iter()
            .find_map(|grant| match &grant.resource {
                Resource::Association { binding } => Some(binding.as_ref()),
                _ => None,
            });
        let mut binding = match &request.request {
            PrRequest::Associations { branch } => {
                let capture = self
                    .resolver
                    .capture(&request.repository, branch.as_deref())
                    .await?;
                if let Some(previous) = previous.filter(|binding| {
                    binding.viewed_branch == *branch
                        && binding.capture.common_storage == capture.common_storage
                        && binding.capture.config_fingerprint == capture.config_fingerprint
                }) {
                    let mut binding = previous.clone();
                    binding.capture = capture;
                    binding
                } else {
                    AssociationBinding {
                        capture,
                        viewed_branch: branch.clone(),
                        mapping: None,
                        known: Vec::new(),
                        selected: None,
                    }
                }
            }
            PrRequest::MapHead {
                owner,
                repository,
                head_ref,
                ..
            } => {
                let mut binding = previous
                    .ok_or_else(|| PrCode::StaleContext.failure())?
                    .clone();
                self.resolver
                    .validate_capture(&request.repository, &binding.capture)
                    .await?;
                let mapping = HeadMappingInput {
                    owner: owner.clone(),
                    repository: repository.clone(),
                    head_ref: head_ref.clone(),
                };
                if binding.mapping.as_ref() != Some(&mapping) {
                    binding.known.clear();
                    binding.selected = None;
                }
                binding.mapping = Some(mapping);
                binding
            }
            _ => return Err(PrCode::InvalidOutput.failure()),
        };
        let observation = self
            .resolver
            .resolve(
                &request.repository,
                binding.capture.clone(),
                &account,
                binding.mapping.as_ref(),
                &binding.known,
            )
            .await?;
        binding.capture = observation.capture.clone();
        Ok(publish_association(observation, binding))
    }
}

impl PullRequestProvider for AssociationProvider {
    fn account(&self) -> Option<HostAccount> {
        self.coordinator.account()
    }

    fn account_changes(&self) -> Option<watch::Receiver<AccountNotice>> {
        Some(self.coordinator.account_changes())
    }

    fn prepare<'a>(
        &'a self,
        repository: &'a SelectedContext,
        request: &'a PrRequest,
        resources: &'a [Grant],
    ) -> Pin<Box<dyn Future<Output = Result<(), Failure>> + Send + 'a>> {
        Box::pin(async move {
            if matches!(request, PrRequest::Release { .. }) {
                return Ok(());
            }
            self.coordinator.observe_account().await?;
            for grant in resources {
                if let Resource::Association { binding } = &grant.resource {
                    self.resolver
                        .validate_capture(repository, &binding.capture)
                        .await?;
                }
            }
            Ok(())
        })
    }

    fn read(
        &self,
        request: ProviderRequest,
    ) -> Pin<Box<dyn Future<Output = Publication> + Send + '_>> {
        Box::pin(async move {
            match request.request {
                PrRequest::Status => Publication {
                    result: PrSuccess::Ready.into(),
                    grants: Vec::new(),
                },
                PrRequest::Associations { .. } | PrRequest::MapHead { .. } => self
                    .association(&request)
                    .await
                    .unwrap_or_else(|failure| Publication {
                        result: PrResult::Failure(failure),
                        grants: Vec::new(),
                    }),
                _ => PrCode::IntegrationUnavailable.into(),
            }
        })
    }
}

fn publish_association(
    observation: AssociationObservation,
    mut binding: AssociationBinding,
) -> Publication {
    // A known historical identity stays reachable, but branch reuse cannot select it again.
    let selected = binding
        .selected
        .as_ref()
        .and_then(|identity| {
            observation
                .candidates
                .iter()
                .find(|candidate| &candidate.identity == identity)
        })
        .or_else(|| {
            (observation.state == ResolutionState::Single)
                .then(|| observation.candidates.first())
                .flatten()
        });
    binding.selected = selected.map(|candidate| candidate.identity.clone());
    if let Some(candidate) = selected {
        if !binding
            .known
            .iter()
            .any(|known| known.identity == candidate.identity)
        {
            if binding.known.len() >= 32 {
                binding.known.remove(0);
            }
            binding.known.push(KnownPull {
                identity: candidate.identity.clone(),
                base_repository: candidate.base_repository.clone(),
            });
        }
    }
    let association = Grant::new(Resource::Association {
        binding: Box::new(binding),
    });
    let association_id = association.id().to_owned();
    let mut grants = vec![association];
    let mut selected_candidate_id = None;
    let mut issue = |candidate: &VerifiedCandidate| {
        let grant = Grant::new(Resource::Candidate {
            association_id: association_id.clone(),
            candidate: candidate.clone(),
        });
        let candidate_id = grant.id().to_owned();
        if selected.is_some_and(|selected| selected.identity == candidate.identity) {
            selected_candidate_id = Some(candidate_id.clone());
        }
        grants.push(grant);
        Candidate {
            candidate_id,
            number: candidate.identity.number,
            title: candidate.title.clone(),
            base_repository: candidate.base_repository.clone(),
            base_ref: candidate.base_ref.clone(),
            head_repository: candidate.head_repository.clone(),
            head_ref: candidate.head_ref.clone(),
            lifecycle: candidate.lifecycle,
        }
    };
    let candidates = observation.candidates.iter().map(&mut issue).collect();
    let historical = observation.historical.iter().map(&mut issue).collect();
    let state = match observation.state {
        ResolutionState::Single => AssociationState::Single,
        ResolutionState::None => AssociationState::None,
        ResolutionState::Ambiguous => AssociationState::Ambiguous,
        ResolutionState::Unresolved => AssociationState::Unresolved,
        ResolutionState::Unavailable => AssociationState::Unavailable,
    };
    Publication {
        result: PrSuccess::Association {
            observation: Association {
                association_id,
                branch_label: observation.capture.branch_label,
                state,
                candidates,
                historical,
                selected_candidate_id,
                complete: observation.complete,
                base_repositories: observation.bases,
                head_mappings: observation
                    .heads
                    .into_iter()
                    .map(|head| AssociationHead {
                        repository: head.repository,
                        head_ref: head.head_ref,
                    })
                    .collect(),
                failure: observation.failure,
                observed_at: if observation.observed_at == 0 {
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as u64
                } else { observation.observed_at },
                freshness: observation.freshness,
            },
        }
        .into(),
        grants,
    }
}
