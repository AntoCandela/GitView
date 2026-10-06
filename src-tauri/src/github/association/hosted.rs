//! Verifies hosted identities and bounded candidate scope through shared account-aware demand.
use super::local::{valid_name, validate_ref};
use super::*;
use crate::github::{
    coordinator::{DemandScope, SnapshotVersion},
    model::{Completeness, GithubHost, PrCode},
    transport::GhRead,
};
use serde_json::Value;

impl AssociationResolver {
    pub(super) async fn resolve_hosted(
        &self,
        context: &SelectedContext,
        capture: LocalCapture,
        account: &HostAccount,
        mapping: Option<&HeadMappingInput>,
        known: &[KnownPull],
    ) -> Result<AssociationObservation, Failure> {
        self.validate_capture(context, &capture).await?;
        self.check_account(account)?;
        if known.len() > 32 {
            return Err(PrCode::ResourceLimit.failure());
        }
        let mut local_heads = capture.heads.clone();
        let mut names = capture.repositories.clone();
        if let Some(mapping) = mapping {
            if !valid_name(&mapping.owner) || !valid_name(&mapping.repository) {
                return Err(PrCode::UnresolvedMapping.failure());
            }
            validate_ref(&mapping.head_ref)?;
            let name = RepositoryName {
                owner: mapping.owner.clone(),
                name: mapping.repository.clone(),
            };
            if !names.contains(&name) {
                names.push(name.clone());
            }
            local_heads = vec![LocalHead {
                repository: name,
                head_ref: mapping.head_ref.clone(),
            }];
        }
        let mut observation = AssociationObservation {
            capture: capture.clone(),
            bases: Vec::new(),
            heads: Vec::new(),
            candidates: Vec::new(),
            historical: Vec::new(),
            state: ResolutionState::Unresolved,
            complete: mapping.is_some() || !capture.unresolved,
            failure: None,
            observed_at: 0,
            freshness: Freshness::Fresh,
        };
        let mut verified = Vec::new();
        let mut parents = Vec::new();
        for name in &names {
            let result = self
                .page(
                    &capture,
                    account,
                    None,
                    GhRead::ReadRepository {
                        owner: name.owner.clone(),
                        repository: name.name.clone(),
                    },
                )
                .await;
            match result {
                Ok(page) => {
                    observation.observed_at = observation.observed_at.max(page.observed_at);
                    let repository = parse_repository(&page.body)?;
                    if page
                        .body
                        .get("fork")
                        .and_then(Value::as_bool)
                        .ok_or_else(|| PrCode::InvalidOutput.failure())?
                    {
                        let parent = parse_repository(&page.body["parent"])?;
                        if !parents.iter().any(|p: &GithubRepository| p.id == parent.id) {
                            parents.push(parent);
                        }
                    }
                    if !observation.bases.iter().any(|r| r.id == repository.id) {
                        observation.bases.push(repository.clone());
                    }
                    verified.push((name.clone(), repository));
                }
                Err(failure) => record_failure(&mut observation, failure),
            }
        }
        for parent in parents {
            if observation.bases.iter().any(|r| r.id == parent.id) {
                continue;
            }
            match self
                .page(
                    &capture,
                    account,
                    None,
                    GhRead::ReadRepository {
                        owner: parent.owner.clone(),
                        repository: parent.name.clone(),
                    },
                )
                .await
            {
                Ok(page) => {
                    let verified_parent = parse_repository(&page.body)?;
                    if verified_parent.id != parent.id {
                        return Err(PrCode::InvalidOutput.failure());
                    }
                    observation.observed_at = observation.observed_at.max(page.observed_at);
                    observation.bases.push(verified_parent);
                }
                Err(failure) => record_failure(&mut observation, failure),
            }
        }
        for head in local_heads {
            if let Some((_, repository)) =
                verified.iter().find(|(name, _)| name == &head.repository)
            {
                if !observation
                    .heads
                    .iter()
                    .any(|h| h.repository.id == repository.id && h.head_ref == head.head_ref)
                {
                    observation.heads.push(VerifiedHead {
                        repository: repository.clone(),
                        head_ref: head.head_ref,
                    });
                }
            } else {
                observation.complete = false;
            }
        }
        // Only a visible first page per identified query is demanded. More/limited is explicit;
        // discovery never drains unattended provider pagination to claim global absence.
        let mut queries = 0;
        for base in observation.bases.clone() {
            for head in observation.heads.clone() {
                queries += 1;
                if queries > 32 {
                    record_failure(&mut observation, PrCode::ResourceLimit.failure());
                    break;
                }
                let result = self
                    .page(
                        &capture,
                        account,
                        None,
                        GhRead::ListPulls {
                            owner: base.owner.clone(),
                            repository: base.name.clone(),
                            head: Some(format!("{}:{}", head.repository.owner, head.head_ref)),
                            page: 1,
                        },
                    )
                    .await;
                match result {
                    Ok(page) => {
                        observation.observed_at = observation.observed_at.max(page.observed_at);
                        if !matches!(page.completeness, Completeness::Complete) {
                            observation.complete = false;
                        }
                        let items = page
                            .body
                            .as_array()
                            .ok_or_else(|| PrCode::InvalidOutput.failure())?;
                        if items.len() > 100 {
                            return Err(PrCode::ResourceLimit.failure());
                        }
                        for item in items {
                            let candidate = parse_candidate(item)?;
                            if candidate.base_repository.id != base.id {
                                return Err(PrCode::InvalidOutput.failure());
                            }
                            if matches!(candidate.lifecycle, Lifecycle::Open)
                                && candidate
                                    .head_repository
                                    .as_ref()
                                    .is_some_and(|r| r.id == head.repository.id)
                                && candidate.head_ref.as_deref() == Some(&head.head_ref)
                                && !observation
                                    .candidates
                                    .iter()
                                    .any(|c| c.identity == candidate.identity)
                            {
                                if observation.candidates.len() >= 2000 {
                                    return Err(PrCode::ResourceLimit.failure());
                                }
                                observation.candidates.push(candidate);
                            }
                        }
                    }
                    Err(failure) => record_failure(&mut observation, failure),
                }
            }
        }
        for previous in known {
            if previous.identity.host != "github.com"
                || previous.identity.base_repository_id != previous.base_repository.id
            {
                return Err(PrCode::InvalidOutput.failure());
            }
            let base = &previous.base_repository;
            match self
                .page(
                    &capture,
                    account,
                    Some(previous.identity.clone()),
                    GhRead::ReadPull {
                        owner: base.owner.clone(),
                        repository: base.name.clone(),
                        number: previous.identity.number,
                    },
                )
                .await
            {
                Ok(page) => {
                    let candidate = parse_candidate(&page.body)?;
                    if candidate.identity != previous.identity {
                        return Err(PrCode::InvalidOutput.failure());
                    }
                    observation.observed_at = observation.observed_at.max(page.observed_at);
                    // Known identity remains reachable independently of a reused branch label.
                    if !observation
                        .candidates
                        .iter()
                        .any(|c| c.identity == candidate.identity)
                    {
                        observation.historical.push(candidate);
                    }
                }
                Err(failure) => record_failure(&mut observation, failure),
            }
        }
        if let Err(failure) = self.check_account(account) {
            return Err(if failure.code == PrCode::AuthUnavailable {
                observation.failure.clone().unwrap_or(failure)
            } else {
                failure
            });
        }
        self.validate_capture(context, &capture).await?;
        observation.state = if observation.failure.is_some() {
            ResolutionState::Unavailable
        } else if observation.heads.is_empty() {
            ResolutionState::Unresolved
        } else if !observation.complete
            || observation.candidates.len() > 1
            || observation.heads.len() > 1
        {
            ResolutionState::Ambiguous
        } else if observation.candidates.len() == 1 {
            ResolutionState::Single
        } else {
            ResolutionState::None
        };
        Ok(observation)
    }
    fn check_account(&self, account: &HostAccount) -> Result<(), Failure> {
        let notice = self.coordinator.account_changes();
        let current = notice.borrow();
        if current.account.as_ref() != Some(account) {
            return Err(PrCode::StaleContext.failure());
        }
        if !current.authorization_known {
            return Err(PrCode::AuthUnavailable.failure());
        }
        Ok(())
    }
    async fn page(
        &self,
        capture: &LocalCapture,
        account: &HostAccount,
        identity: Option<PrIdentity>,
        read: GhRead,
    ) -> Result<Arc<crate::github::coordinator::CachedPage>, Failure> {
        self.check_account(account)?;
        let scope = DemandScope {
            common_storage: capture.common_storage.clone(),
            config_fingerprint: capture.config_fingerprint.clone(),
            mapped_branch: capture.branch_label.clone(),
            identity,
            version: SnapshotVersion {
                revision: 0,
                base_oid: None,
                head_oid: None,
                source: None,
                base_kind: None,
            },
        };
        let mut lease = self.coordinator.request(scope, read)?;
        loop {
            let snapshot = lease.snapshot();
            if !snapshot.loading {
                if let Some(failure) = snapshot.failure {
                    return Err(failure);
                }
                self.check_account(account)?;
                if snapshot.retention_limited {
                    return Err(PrCode::ResourceLimit.failure());
                }
                if snapshot.stale {
                    return Err(PrCode::Network.failure());
                }
                return snapshot.page.ok_or_else(|| PrCode::InvalidOutput.failure());
            }
            lease.changed().await;
        }
    }
}
fn record_failure(observation: &mut AssociationObservation, failure: Failure) {
    observation.complete = false;
    if observation.failure.is_none() {
        observation.failure = Some(failure);
    }
}
fn string(value: &Value, field: &str, max: usize) -> Result<String, Failure> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| PrCode::InvalidOutput.failure())
}
fn parse_repository(value: &Value) -> Result<GithubRepository, Failure> {
    let id = value
        .get("id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or_else(|| PrCode::InvalidOutput.failure())?
        .to_string();
    let owner = string(&value["owner"], "login", 100)?;
    let name = string(value, "name", 100)?;
    if !valid_name(&owner) || !valid_name(&name) {
        return Err(PrCode::InvalidOutput.failure());
    }
    Ok(GithubRepository {
        id,
        host: GithubHost::GithubCom,
        url: format!("https://github.com/{owner}/{name}"),
        owner,
        name,
    })
}
fn parse_candidate(value: &Value) -> Result<VerifiedCandidate, Failure> {
    let base_repository = parse_repository(&value["base"]["repo"])?;
    let head_repository = if value["head"]["repo"].is_null() {
        None
    } else {
        Some(parse_repository(&value["head"]["repo"])?)
    };
    let head_ref = if value["head"]["ref"].is_null() {
        None
    } else {
        Some(string(&value["head"], "ref", 512)?)
    };
    let number = value
        .get("number")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| PrCode::InvalidOutput.failure())?;
    let lifecycle = match value.get("state").and_then(Value::as_str) {
        Some("open") => Lifecycle::Open,
        Some("closed")
            if value.get("merged_at").is_some_and(|v| !v.is_null())
                || value.get("merged").and_then(Value::as_bool) == Some(true) =>
        {
            Lifecycle::Merged
        }
        Some("closed") => Lifecycle::Closed,
        _ => return Err(PrCode::InvalidOutput.failure()),
    };
    Ok(VerifiedCandidate {
        identity: PrIdentity {
            host: "github.com".into(),
            base_repository_id: base_repository.id.clone(),
            number,
        },
        base_repository,
        base_ref: string(&value["base"], "ref", 512)?,
        head_repository,
        head_ref,
        lifecycle,
        title: string(value, "title", 4096)?,
    })
}
