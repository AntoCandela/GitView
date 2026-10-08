//! Exercises shared demand and account isolation with deterministic provider gates.
use super::*;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
struct Fake {
    calls: AtomicUsize,
    probes: AtomicUsize,
    dropped: AtomicUsize,
    block: AtomicBool,
    gate: Semaphore,
    user: Mutex<String>,
    replies: Mutex<VecDeque<Result<Value, Failure>>>,
    auth_errors: Mutex<VecDeque<Failure>>,
    changed: tokio::sync::Notify,
    auth_block: AtomicBool,
    auth_gate: Semaphore,
    next_rate: Mutex<Option<(u64, u64)>>,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            probes: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            block: AtomicBool::new(false),
            gate: Semaphore::new(0),
            user: Mutex::new("account-a".into()),
            replies: Mutex::new(VecDeque::new()),
            auth_errors: Mutex::new(VecDeque::new()),
            changed: tokio::sync::Notify::new(),
            auth_block: AtomicBool::new(false),
            auth_gate: Semaphore::new(0),
            next_rate: Mutex::new(None),
        }
    }
}
struct Pending<'a>(&'a Fake);
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
        self.0.changed.notify_one();
    }
}
impl ReadTransport for Fake {
    fn observe(&self) -> ReadFuture<'_, Result<AccountObservation, Failure>> {
        Box::pin(async {
            self.probes.fetch_add(1, Ordering::SeqCst);
            self.changed.notify_one();
            if self.auth_block.load(Ordering::SeqCst) {
                self.auth_gate.acquire().await.unwrap().forget();
            }
            if let Some(error) = self.auth_errors.lock().pop_front() {
                return Err(error);
            }
            Ok(AccountObservation {
                provider_user_id: self.user.lock().clone(),
            })
        })
    }
    fn read(&self, _: GhRead) -> ReadFuture<'_, Result<ApiResponse, Failure>> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.changed.notify_one();
            let _pending = Pending(self);
            if self.block.load(Ordering::SeqCst) {
                self.gate.acquire().await.unwrap().forget();
            }
            let body = self
                .replies
                .lock()
                .pop_front()
                .unwrap_or_else(|| Ok(json!({"private":"body"})))?;
            let mut response = ApiResponse {
                status: 200, has_next: false,
                rate: Default::default(),
                body,
            };
            if let Some((remaining, retry_at)) = self.next_rate.lock().take() {
                response.rate.remaining = Some(remaining);
                response.rate.retry_at = Some(retry_at);
            }
            Ok(response)
        })
    }
}
fn scope() -> DemandScope {
    DemandScope {
        common_storage: std::env::temp_dir().join("native-fixture-common"),
        config_fingerprint: "config-a".into(),
        mapped_branch: Some("branch".into()),
        identity: Some(crate::github::PrIdentity {
            host: "github.com".into(),
            base_repository_id: "repository-id".into(),
            number: 1,
        }),
        version: SnapshotVersion {
            revision: 1,
            base_oid: Some("a".repeat(40)),
            head_oid: Some("b".repeat(40)),
            source: None,
            base_kind: None,
        },
    }
}
fn read() -> GhRead {
    GhRead::ReadPull {
        owner: "owner".into(),
        repository: "repo".into(),
        number: 1,
    }
}
async fn setup() -> (Arc<Fake>, PrDemandCoordinator) {
    let fake = Arc::new(Fake::default());
    let coordinator = PrDemandCoordinator::with_transport(fake.clone());
    coordinator.observe_account().await.unwrap();
    (fake, coordinator)
}
async fn until(fake: &Fake, condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            fake.changed.notified().await;
        }
    })
    .await
    .expect("provider condition must become observable");
}
async fn complete(lease: &mut DemandLease) -> DemandSnapshot {
    loop {
        let snapshot = lease.snapshot();
        if !snapshot.loading {
            return snapshot;
        }
        lease.changed().await;
    }
}
#[tokio::test]
async fn github_coordinator_identical_consumers_share_one_pending_provider_read() {
    let (fake, coordinator) = setup().await;
    fake.block.store(true, Ordering::SeqCst);
    let first = coordinator.request(scope(), read()).unwrap();
    let mut survivor = coordinator.request(scope(), read()).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    drop(first);
    assert_eq!(fake.dropped.load(Ordering::SeqCst), 0);
    fake.gate.add_permits(1);
    assert!(complete(&mut survivor).await.page.is_some());
}

#[tokio::test]
async fn github_coordinator_last_release_cancels_and_hidden_demand_resumes_without_polling() {
    let (fake, coordinator) = setup().await;
    fake.block.store(true, Ordering::SeqCst);
    let lease = coordinator.request(scope(), read()).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    coordinator.set_visible(lease.id, false).unwrap();
    until(&fake, || fake.dropped.load(Ordering::SeqCst) == 1).await;
    assert!(!lease.snapshot().loading);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    coordinator.set_visible(lease.id, true).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 2).await;
    coordinator.release(lease.id);
    until(&fake, || fake.dropped.load(Ordering::SeqCst) == 2).await;
    assert!(lease.snapshot().page.is_none());
}

#[tokio::test]
async fn github_coordinator_account_switch_discards_pending_and_cached_private_data() {
    let (fake, coordinator) = setup().await;
    let mut cached = coordinator.request(scope(), read()).unwrap();
    assert!(complete(&mut cached).await.page.is_some());
    let old_epoch = coordinator.account().unwrap().account_epoch;
    let notices = coordinator.account_changes();
    fake.block.store(true, Ordering::SeqCst);
    coordinator.refresh(cached.id).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 2).await;
    *fake.user.lock() = "account-b".into();
    fake.gate.add_permits(1);
    assert!(complete(&mut cached).await.page.is_none());
    assert_eq!(
        notices.borrow().account.as_ref().unwrap().provider_user_id,
        "account-b"
    );
    assert!(notices.borrow().authorization_known);
    assert!(coordinator.account().unwrap().account_epoch > old_epoch);
    fake.block.store(false, Ordering::SeqCst);
    fake.replies
        .lock()
        .push_back(Ok(json!({"private":"account-b-value"})));
    let mut new = coordinator.request(scope(), read()).unwrap();
    assert_eq!(
        complete(&mut new).await.page.unwrap().body["private"],
        "account-b-value"
    );
}

#[tokio::test]
async fn github_coordinator_concurrent_initial_discovery_shares_one_probe_and_invalidation_rejects_late_identity(
) {
    let fake = Arc::new(Fake::default());
    fake.auth_block.store(true, Ordering::SeqCst);
    let coordinator = Arc::new(PrDemandCoordinator::with_transport(fake.clone()));
    let first = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.observe_account().await })
    };
    until(&fake, || fake.probes.load(Ordering::SeqCst) == 1).await;
    let second = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.observe_account().await })
    };
    // The held auth mutex establishes the second caller's overlap without a time delay.
    tokio::task::yield_now().await;
    fake.auth_gate.add_permits(1);
    assert_eq!(
        first.await.unwrap().unwrap().account_epoch,
        second.await.unwrap().unwrap().account_epoch
    );
    assert_eq!(fake.probes.load(Ordering::SeqCst), 1);
    let late = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.observe_account().await })
    };
    until(&fake, || fake.probes.load(Ordering::SeqCst) == 2).await;
    coordinator.invalidate(Invalidation::Account);
    fake.auth_gate.add_permits(1);
    assert!(matches!(
        late.await.unwrap(),
        Err(Failure {
            code: PrCode::StaleContext,
            ..
        })
    ));
    assert!(coordinator.account().is_none());
}

#[tokio::test]
async fn github_coordinator_unknown_auth_blocks_unseen_cross_key_cache_and_keeps_seen_data_stale() {
    let (fake, coordinator) = setup().await;
    let mut first = coordinator.request(scope(), read()).unwrap();
    let original = complete(&mut first).await.page.unwrap().observed_at;
    let mut unseen = coordinator
        .request(
            scope(),
            GhRead::ReadRepository {
                owner: "owner".into(),
                repository: "repo".into(),
            },
        )
        .unwrap();
    // Let authorization finish without reading the second lease's body.
    tokio::time::timeout(Duration::from_secs(5), async {
        while unseen
            .inner
            .upgrade()
            .unwrap()
            .state
            .lock()
            .entries
            .values()
            .any(|entry| entry.job.is_some())
        {
            unseen.changed().await;
        }
    })
    .await
    .unwrap();
    fake.auth_errors.lock().push_back(PrCode::Network.failure());
    assert!(coordinator.observe_account().await.is_err());
    let retained = first.snapshot();
    assert!(retained.stale);
    assert_eq!(retained.page.unwrap().observed_at, original);
    assert!(unseen.snapshot().page.is_none());
    assert!(!coordinator.account_changes().borrow().authorization_known);
    assert_eq!(first.snapshot().failure.unwrap().code, PrCode::Network);
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_ttl_and_explicit_refresh_preserve_observation_times() {
    let (fake, coordinator) = setup().await;
    let mut first = coordinator.request(scope(), read()).unwrap();
    let time = complete(&mut first).await.page.unwrap().observed_at;
    tokio::time::advance(Duration::from_secs(59)).await;
    let mut reused = coordinator.request(scope(), read()).unwrap();
    assert_eq!(complete(&mut reused).await.page.unwrap().observed_at, time);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    coordinator.refresh(first.id).unwrap();
    assert!(complete(&mut first).await.page.unwrap().observed_at > time);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    tokio::time::advance(Duration::from_secs(61)).await;
    assert!(first.snapshot().stale);
    let mut expired = coordinator.request(scope(), read()).unwrap();
    complete(&mut expired).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_only_confirmed_first_empty_pull_lookup_gets_longer_ttl() {
    let (fake, coordinator) = setup().await;
    fake.replies.lock().extend([Ok(json!([])), Ok(json!([]))]);
    let list = GhRead::ListPulls {
        owner: "owner".into(),
        repository: "repo".into(),
        head: Some("fork:branch".into()),
        page: 1,
    };
    let mut lease = coordinator.request(scope(), list.clone()).unwrap();
    assert!(complete(&mut lease).await.page.unwrap().confirmed_negative);
    tokio::time::advance(Duration::from_secs(179)).await;
    let mut second = coordinator.request(scope(), list.clone()).unwrap();
    complete(&mut second).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_secs(2)).await;
    let mut third = coordinator.request(scope(), list).unwrap();
    complete(&mut third).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    let response = ApiResponse {
        status: 200, has_next: false,
        rate: Default::default(),
        body: json!([]),
    };
    let continuation = cache::decode(
        &GhRead::ListPulls {
            owner: "owner".into(),
            repository: "repo".into(),
            head: None,
            page: 2,
        },
        response,
        0,
    )
    .unwrap();
    assert!(!continuation.confirmed_negative);
    assert!(matches!(
        continuation.completeness,
        crate::github::model::Completeness::Complete
    ));
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_retries_only_twice_and_hidden_retry_never_runs() {
    let (fake, coordinator) = setup().await;
    fake.replies
        .lock()
        .extend((0..3).map(|_| Err(PrCode::Network.failure())));
    let mut lease = coordinator.request(scope(), read()).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    tokio::time::advance(Duration::from_millis(999)).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_millis(2)).await;
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 2).await;
    tokio::time::advance(Duration::from_millis(2999)).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    tokio::time::advance(Duration::from_millis(2)).await;
    let result = complete(&mut lease).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
    assert_eq!(result.failure.unwrap().code, PrCode::Network);
    fake.replies
        .lock()
        .push_back(Err(PrCode::Network.failure()));
    coordinator.refresh(lease.id).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 4).await;
    coordinator.set_visible(lease.id, false).unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 4);
    assert!(!lease.snapshot().loading);
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_global_throttle_blocks_refresh_and_auth_probes_until_guidance() {
    let (fake, coordinator) = setup().await;
    let retry_at = coordinator.inner.now() + 10_000;
    fake.replies.lock().push_back(Err(Failure {
        retry_at: Some(retry_at),
        ..PrCode::RateLimited.failure()
    }));
    let mut lease = coordinator.request(scope(), read()).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    let probes = fake.probes.load(Ordering::SeqCst);
    assert!(matches!(
        coordinator.observe_account().await,
        Err(Failure {
            code: PrCode::RateLimited,
            ..
        })
    ));
    coordinator.refresh(lease.id).unwrap();
    tokio::time::advance(Duration::from_secs(9)).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.probes.load(Ordering::SeqCst), probes);
    assert_eq!(lease.snapshot().failure.unwrap().retry_at, Some(retry_at));
    tokio::time::advance(Duration::from_millis(1001)).await;
    assert!(complete(&mut lease).await.page.is_some());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn github_coordinator_auth_access_and_missing_repository_revoke_private_values() {
    for code in [
        PrCode::AuthRequired,
        PrCode::AuthUnavailable,
        PrCode::AccessDenied,
        PrCode::RepositoryUnavailable,
    ] {
        let (fake, coordinator) = setup().await;
        let mut lease = coordinator.request(scope(), read()).unwrap();
        assert!(complete(&mut lease).await.page.is_some());
        fake.replies.lock().push_back(Err(code.failure()));
        coordinator.refresh(lease.id).unwrap();
        let outcome = complete(&mut lease).await;
        assert!(outcome.page.is_none());
        assert_eq!(outcome.failure.unwrap().code, code);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn github_coordinator_context_and_snapshot_invalidation_reject_late_results() {
    for invalidation in [
        Invalidation::Repository(scope().common_storage),
        Invalidation::Configuration {
            common_storage: scope().common_storage,
            keep_fingerprint: "replacement".into(),
        },
        Invalidation::Branch {
            common_storage: scope().common_storage,
            branch: scope().mapped_branch,
        },
        Invalidation::Snapshot {
            identity: scope().identity.unwrap(),
            keep: SnapshotVersion {
                revision: 2,
                ..scope().version
            },
        },
    ] {
        let (fake, coordinator) = setup().await;
        fake.block.store(true, Ordering::SeqCst);
        let lease = coordinator.request(scope(), read()).unwrap();
        until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
        coordinator.invalidate(invalidation);
        until(&fake, || fake.dropped.load(Ordering::SeqCst) == 1).await;
        assert!(lease.snapshot().page.is_none());
        assert_eq!(lease.snapshot().failure.unwrap().code, PrCode::StaleContext);
        fake.block.store(false, Ordering::SeqCst);
        fake.replies
            .lock()
            .push_back(Ok(json!({"revision":"replacement"})));
        let mut replacement = coordinator.request(scope(), read()).unwrap();
        assert_eq!(
            complete(&mut replacement).await.page.unwrap().body["revision"],
            "replacement"
        );
        assert!(lease.snapshot().page.is_none());
    }
}

#[tokio::test]
async fn github_coordinator_page_retention_is_bounded_and_eviction_remains_limited() {
    let (fake, coordinator) = setup().await;
    let mut leases = Vec::new();
    for page in 1..=21 {
        fake.replies
            .lock()
            .push_back(Ok(json!(vec![json!({"id":page}); 100])));
        let mut lease = coordinator
            .request(
                scope(),
                GhRead::ReadPullFiles {
                    owner: "owner".into(),
                    repository: "repo".into(),
                    number: 1,
                    page,
                },
            )
            .unwrap();
        assert!(complete(&mut lease).await.page.is_some());
        leases.push(lease);
    }
    let state = coordinator.inner.state.lock();
    assert_eq!(
        state
            .entries
            .values()
            .filter(|entry| entry.page.is_some())
            .count(),
        20
    );
    assert_eq!(
        state
            .entries
            .values()
            .filter_map(|entry| entry.page.as_ref())
            .map(|page| page.items)
            .sum::<usize>(),
        2000
    );
    drop(state);
    assert!(leases[0].snapshot().page.is_none());
    assert!(leases[20].snapshot().retention_limited);
    assert_eq!(
        leases[0].snapshot().failure.unwrap().code,
        PrCode::ResourceLimit
    );
}

#[tokio::test]
async fn github_coordinator_decoded_budget_evicts_values_and_metadata_is_finite() {
    let (fake, coordinator) = setup().await;
    let mut leases = Vec::new();
    for number in 1..=34 {
        fake.replies
            .lock()
            .push_back(Ok(json!({"body":"x".repeat(1024*1024)})));
        let mut lease = coordinator
            .request(
                scope(),
                GhRead::ReadPull {
                    owner: "owner".into(),
                    repository: "repo".into(),
                    number,
                },
            )
            .unwrap();
        complete(&mut lease).await;
        leases.push(lease);
    }
    assert!(coordinator.inner.state.lock().bytes <= MAX_BYTES);
    assert!(leases[0].snapshot().page.is_none());
    while leases.len() < MAX_CONSUMERS {
        leases.push(coordinator.request(scope(), read()).unwrap());
    }
    assert!(matches!(
        coordinator.request(scope(), read()),
        Err(Failure {
            code: PrCode::ResourceLimit,
            ..
        })
    ));
}

#[tokio::test]
async fn github_coordinator_native_shutdown_settles_owned_jobs() {
    let (fake, coordinator) = setup().await;
    fake.block.store(true, Ordering::SeqCst);
    let work = crate::native_work::NativeWork::default();
    let mut lease = work
        .scope(async { coordinator.request(scope(), read()).unwrap() })
        .await;
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    work.close();
    until(&fake, || fake.dropped.load(Ordering::SeqCst) == 1).await;
    work.drain().await;
    assert!(!complete(&mut lease).await.loading);
    assert!(lease.snapshot().page.is_none());
}

#[tokio::test]
async fn github_coordinator_unknown_auth_cannot_expose_a_replacement_page_not_seen_by_consumer() {
    let (fake, coordinator) = setup().await;
    let mut lease = coordinator.request(scope(), read()).unwrap();
    assert!(complete(&mut lease).await.page.is_some());
    fake.replies
        .lock()
        .push_back(Ok(json!({"private":"unseen-replacement"})));
    coordinator.refresh(lease.id).unwrap();
    loop {
        let loading = coordinator
            .inner
            .state
            .lock()
            .entries
            .values()
            .any(|entry| entry.job.is_some());
        if !loading {
            break;
        }
        lease.changed().await;
    }
    fake.auth_errors.lock().push_back(PrCode::Network.failure());
    assert!(coordinator.observe_account().await.is_err());
    assert!(
        lease.snapshot().page.is_none(),
        "unknown authentication cannot reveal a replacement the consumer never observed"
    );
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_distinct_pages_of_one_collection_never_read_concurrently() {
    let (fake, coordinator) = setup().await;
    fake.block.store(true, Ordering::SeqCst);
    fake.replies
        .lock()
        .extend([Ok(json!([{"page":1}])), Ok(json!([{"page":2}]))]);
    let page = |page| GhRead::ReadPullFiles {
        owner: "owner".into(),
        repository: "repo".into(),
        number: 1,
        page,
    };
    let mut first = coordinator.request(scope(), page(1)).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    let mut second = coordinator.request(scope(), page(2)).unwrap();
    // Paused time advances only after runnable jobs have parked at their gates.
    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        until(&fake, || fake.calls.load(Ordering::SeqCst) >= 2)
    )
    .await
    .is_err());
    fake.gate.add_permits(1);
    assert!(complete(&mut first).await.page.is_some());
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 2).await;
    assert!(second.snapshot().loading);
    fake.gate.add_permits(1);
    assert!(complete(&mut second).await.page.is_some());
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_successful_exhausted_rate_header_defers_the_following_identity_probe() {
    let (fake, coordinator) = setup().await;
    let retry_at = coordinator.inner.now() + 10_000;
    *fake.next_rate.lock() = Some((0, retry_at));
    let mut lease = coordinator.request(scope(), read()).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 1).await;
    assert_eq!(fake.probes.load(Ordering::SeqCst), 2);
    assert!(lease.snapshot().page.is_none());
    assert_eq!(lease.snapshot().failure.unwrap().retry_at, Some(retry_at));
    assert!(matches!(
        coordinator.observe_account().await,
        Err(Failure {
            code: PrCode::RateLimited,
            ..
        })
    ));
    tokio::time::advance(Duration::from_secs(9)).await;
    assert_eq!(fake.probes.load(Ordering::SeqCst), 2);
    tokio::time::advance(Duration::from_millis(1001)).await;
    assert!(complete(&mut lease).await.page.is_some());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_offline_refresh_keeps_only_identified_already_seen_page() {
    let (fake, coordinator) = setup().await;
    let mut lease = coordinator.request(scope(), read()).unwrap();
    let old = complete(&mut lease).await.page.unwrap();
    fake.replies
        .lock()
        .extend((0..3).map(|_| Err(PrCode::Network.failure())));
    coordinator.refresh(lease.id).unwrap();
    let outcome = complete(&mut lease).await;
    assert!(outcome.stale);
    assert_eq!(outcome.failure.unwrap().code, PrCode::Network);
    let page = outcome.page.unwrap();
    assert_eq!(page.observation_id, old.observation_id);
    assert_eq!(page.observed_at, old.observed_at);
}

#[test]
fn github_coordinator_keys_isolate_routing_context_and_immutable_endpoints() {
    use crate::github::model::{BaseKind, ComparisonSource};
    let key = Key {
        scope: scope(),
        read: read(),
        user: "account".into(),
        epoch: 1,
    };
    let mut changed = key.clone();
    changed.scope.common_storage.push("other");
    assert!(changed != key);
    changed = key.clone();
    changed.scope.config_fingerprint = "other".into();
    assert!(changed != key);
    changed = key.clone();
    changed.scope.mapped_branch = Some("other".into());
    assert!(changed != key);
    changed = key.clone();
    changed.user = "other".into();
    assert!(changed != key);
    changed = key.clone();
    changed.epoch = 2;
    assert!(changed != key);
    let mut immutable = key.clone();
    immutable.scope.version.source = Some(ComparisonSource::GithubPatch);
    immutable.scope.version.base_kind = Some(BaseKind::Provider);
    changed = immutable.clone();
    changed.scope.version.source = Some(ComparisonSource::LocalGit);
    assert!(changed != immutable);
    changed = immutable.clone();
    changed.scope.version.base_oid = Some("c".repeat(40));
    assert!(changed != immutable);
    changed = immutable.clone();
    changed.scope.version.head_oid = Some("d".repeat(40));
    assert!(changed != immutable);
    changed = immutable.clone();
    changed.scope.version.revision += 1;
    assert!(changed != immutable);
    changed = immutable.clone();
    changed.scope.version.base_kind = Some(BaseKind::Parent);
    assert!(changed != immutable);
}

#[tokio::test]
async fn github_coordinator_identityless_repository_404_revokes_common_storage_private_cache_only()
{
    let (fake, coordinator) = setup().await;
    let mut same = coordinator.request(scope(), read()).unwrap();
    assert!(complete(&mut same).await.page.is_some());
    let mut different_scope = scope();
    different_scope.common_storage.push("different-repository");
    let mut different = coordinator.request(different_scope, read()).unwrap();
    let kept = complete(&mut different).await.page.unwrap().observation_id;
    let mut lookup_scope = scope();
    lookup_scope.identity = None;
    fake.replies
        .lock()
        .push_back(Err(PrCode::RepositoryUnavailable.failure()));
    let mut lookup = coordinator
        .request(
            lookup_scope,
            GhRead::ReadRepository {
                owner: "owner".into(),
                repository: "repo".into(),
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut lookup).await.failure.unwrap().code,
        PrCode::RepositoryUnavailable
    );
    assert!(
        same.snapshot().page.is_none(),
        "an identity-less inaccessible repository lookup must revoke its existing private PR cache"
    );
    assert_eq!(
        same.snapshot().failure.unwrap().code,
        PrCode::RepositoryUnavailable
    );
    assert_eq!(different.snapshot().page.unwrap().observation_id, kept);
}

#[tokio::test(start_paused = true)]
async fn github_coordinator_auth_observation_notifies_leases_of_unknown_auth_and_recovery() {
    let (fake, coordinator) = setup().await;
    let mut lease = coordinator.request(scope(), read()).unwrap();
    assert!(complete(&mut lease).await.page.is_some());
    lease.changes.borrow_and_update();
    fake.auth_errors.lock().push_back(PrCode::Network.failure());
    assert!(coordinator.observe_account().await.is_err());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), lease.changed())
            .await
            .is_ok(),
        "lease snapshot changes must notify its subscriber, not only account subscribers"
    );
    assert!(lease.snapshot().stale);
    lease.changes.borrow_and_update();
    coordinator.observe_account().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), lease.changed())
            .await
            .is_ok(),
        "same-account authorization recovery also changes lease freshness"
    );
    assert!(!lease.snapshot().stale);
}

#[tokio::test]
async fn github_coordinator_refresh_during_cache_validation_forces_one_read_without_duplicate_network_work(
) {
    let (fake, coordinator) = setup().await;
    let mut first = coordinator.request(scope(), read()).unwrap();
    assert!(complete(&mut first).await.page.is_some());
    let probes = fake.probes.load(Ordering::SeqCst);
    fake.auth_block.store(true, Ordering::SeqCst);
    let mut pending = coordinator.request(scope(), read()).unwrap();
    until(&fake, || fake.probes.load(Ordering::SeqCst) == probes + 1).await;
    coordinator.refresh(pending.id).unwrap();
    fake.auth_block.store(false, Ordering::SeqCst);
    fake.auth_gate.add_permits(1);
    assert!(complete(&mut pending).await.page.is_some());
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        2,
        "manual refresh must bypass fresh cache even when its auth-validation job already exists"
    );
    fake.block.store(true, Ordering::SeqCst);
    coordinator.refresh(pending.id).unwrap();
    until(&fake, || fake.calls.load(Ordering::SeqCst) == 3).await;
    coordinator.refresh(pending.id).unwrap();
    fake.gate.add_permits(1);
    assert!(complete(&mut pending).await.page.is_some());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}
