//! Shares account-scoped gh demand; cached provider bodies never confer session authority.
//! Provider composition must reconcile account notifications with the service before publication.
mod cache;
mod scope;
mod work;

use super::{
    model::{Failure, GithubHost, PrCode},
    service::HostAccount,
    transport::{AccountObservation, ApiResponse, GhRead, GhReadAdapter, GhReply},
};
pub(crate) use cache::CachedPage;
use parking_lot::Mutex;
use scope::{CollectionKey, Key};
pub(crate) use scope::{DemandScope, Invalidation, SnapshotVersion};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{watch, Semaphore};
use uuid::Uuid;

const MAX_ENTRIES: usize = 128;
const MAX_CONSUMERS: usize = 512;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGES: usize = 20;
const MAX_ITEMS: usize = 2000;
const POSITIVE_TTL: Duration = Duration::from_secs(60);
const NEGATIVE_TTL: Duration = Duration::from_secs(180);

type ReadFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
trait ReadTransport: Send + Sync {
    fn observe(&self) -> ReadFuture<'_, Result<AccountObservation, Failure>>;
    fn read(&self, read: GhRead) -> ReadFuture<'_, Result<ApiResponse, Failure>>;
}
impl ReadTransport for GhReadAdapter {
    fn observe(&self) -> ReadFuture<'_, Result<AccountObservation, Failure>> {
        Box::pin(self.observe_account())
    }
    fn read(&self, read: GhRead) -> ReadFuture<'_, Result<ApiResponse, Failure>> {
        Box::pin(async move {
            match GhReadAdapter::read(self, read).await? {
                GhReply::Api(response) => Ok(response),
                _ => Err(PrCode::InvalidOutput.failure()),
            }
        })
    }
}

/// Body-free notice. An offline observation does not certify continuing authorization.
#[derive(Clone)]
pub(crate) struct AccountNotice {
    pub account: Option<HostAccount>,
    pub authorization_known: bool,
}
pub(crate) struct DemandSnapshot {
    pub page: Option<Arc<CachedPage>>,
    pub stale: bool,
    pub loading: bool,
    pub failure: Option<Failure>,
    pub retention_limited: bool,
}
struct Entry {
    page: Option<Arc<CachedPage>>,
    observed: tokio::time::Instant,
    bytes: usize,
    used: u64,
    job: Option<tokio::task::AbortHandle>,
    generation: u64,
    force_read: bool,
    authorized: bool,
    failure: Option<Failure>,
}
struct Consumer {
    key: Key,
    visible: bool,
    seen: Option<Uuid>,
    revoked: Option<Failure>,
}
struct Collection {
    gate: Arc<Semaphore>,
    limited: bool,
}
struct State {
    account: Option<HostAccount>,
    epoch: u64,
    auth_revision: u64,
    auth_result: Result<HostAccount, Failure>,
    entries: HashMap<Key, Entry>,
    consumers: HashMap<Uuid, Consumer>,
    collections: HashMap<CollectionKey, Collection>,
    bytes: usize,
    used: u64,
    next_job: u64,
    cooldown: Option<u64>,
    closed: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            account: None,
            epoch: 0,
            auth_revision: 0,
            auth_result: Err(PrCode::AuthUnavailable.failure()),
            entries: HashMap::new(),
            consumers: HashMap::new(),
            collections: HashMap::new(),
            bytes: 0,
            used: 0,
            next_job: 0,
            cooldown: None,
            closed: false,
        }
    }
}
struct Inner {
    state: Mutex<State>,
    transport: Arc<dyn ReadTransport>,
    auth_gate: tokio::sync::Mutex<()>,
    changes: watch::Sender<u64>,
    accounts: watch::Sender<AccountNotice>,
    started: tokio::time::Instant,
    unix_ms: u64,
}

/// Discover the account before service admission. Consume account notices to reconcile service
/// authority on change/revocation; this owner neither issues nor stores renderer handles.
pub(crate) struct PrDemandCoordinator {
    inner: Arc<Inner>,
}
impl PrDemandCoordinator {
    pub(crate) fn new(adapter: GhReadAdapter) -> Self {
        Self::with_transport(Arc::new(adapter))
    }
    fn with_transport(transport: Arc<dyn ReadTransport>) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                transport,
                auth_gate: tokio::sync::Mutex::new(()),
                changes: watch::channel(0).0,
                accounts: watch::channel(AccountNotice {
                    account: None,
                    authorization_known: false,
                })
                .0,
                started: tokio::time::Instant::now(),
                unix_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(i64::MAX as u128) as u64,
            }),
        }
    }
    pub(crate) fn account(&self) -> Option<HostAccount> {
        self.inner.state.lock().account.clone()
    }
    pub(crate) fn account_changes(&self) -> watch::Receiver<AccountNotice> {
        self.inner.accounts.subscribe()
    }
    pub(crate) async fn observe_account(&self) -> Result<HostAccount, Failure> {
        self.inner.observe().await
    }

    /// Scope comes from admitted native context, never renderer decoding. Equal requests share
    /// work; dropping the last visible lease cancels the owned child or retry timer.
    pub(crate) fn request(&self, scope: DemandScope, read: GhRead) -> Result<DemandLease, Failure> {
        scope.validate(&read)?;
        let mut state = self.inner.state.lock();
        if state.closed {
            return Err(PrCode::StaleContext.failure());
        }
        let account = state
            .account
            .clone()
            .ok_or_else(|| PrCode::AuthUnavailable.failure())?;
        let key = Key {
            scope,
            read,
            user: account.provider_user_id,
            epoch: account.account_epoch,
        };
        if state.consumers.len() >= MAX_CONSUMERS {
            return Err(PrCode::ResourceLimit.failure());
        }
        if !state.entries.contains_key(&key) {
            if state.entries.len() >= MAX_ENTRIES {
                state.evict_idle();
            }
            if state.entries.len() >= MAX_ENTRIES {
                return Err(PrCode::ResourceLimit.failure());
            }
            state.entries.insert(
                key.clone(),
                Entry {
                    page: None,
                    observed: tokio::time::Instant::now(),
                    bytes: 0,
                    used: 0,
                    job: None,
                    generation: 0,
                    force_read: false,
                    authorized: false,
                    failure: None,
                },
            );
            state
                .collections
                .entry(key.collection())
                .or_insert_with(|| Collection {
                    gate: Arc::new(Semaphore::new(1)),
                    limited: false,
                });
        }
        let id = Uuid::new_v4();
        state.consumers.insert(
            id,
            Consumer {
                key: key.clone(),
                visible: true,
                seen: None,
                revoked: None,
            },
        );
        self.inner.launch(&mut state, &key, false);
        Ok(DemandLease {
            id,
            inner: Arc::downgrade(&self.inner),
            changes: self.inner.changes.subscribe(),
        })
    }
    pub(crate) fn refresh(&self, id: Uuid) -> Result<(), Failure> {
        let mut state = self.inner.state.lock();
        let key = state
            .consumers
            .get(&id)
            .filter(|c| c.visible && c.revoked.is_none())
            .map(|c| c.key.clone())
            .ok_or_else(|| PrCode::StaleContext.failure())?;
        self.inner.launch(&mut state, &key, true);
        Ok(())
    }
    pub(crate) fn set_visible(&self, id: Uuid, visible: bool) -> Result<(), Failure> {
        let mut state = self.inner.state.lock();
        let consumer = state
            .consumers
            .get_mut(&id)
            .ok_or_else(|| PrCode::StaleContext.failure())?;
        if let Some(failure) = &consumer.revoked {
            return Err(failure.clone());
        }
        let changed = consumer.visible != visible;
        consumer.visible = visible;
        let key = consumer.key.clone();
        if !state.live(&key) {
            state.cancel(&key);
        } else if changed && visible {
            self.inner.launch(&mut state, &key, false);
        }
        self.inner.notify();
        Ok(())
    }
    pub(crate) fn release(&self, id: Uuid) {
        self.inner.release(id);
    }
    pub(crate) fn invalidate(&self, scope: Invalidation) {
        let mut state = self.inner.state.lock();
        if matches!(scope, Invalidation::Account) {
            self.inner
                .revoke_account(&mut state, PrCode::AuthUnavailable.failure());
        } else {
            let keys: Vec<_> = state
                .entries
                .keys()
                .filter(|key| scope.matches(&key.scope))
                .cloned()
                .collect();
            for key in keys {
                state.remove(&key);
            }
            self.inner.notify();
        }
    }
}
impl Drop for PrDemandCoordinator {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock();
        state.closed = true;
        state.clear();
        self.inner.notify();
    }
}

/// Notifications hold no private body; snapshots revalidate current ownership under the lock.
pub(crate) struct DemandLease {
    pub id: Uuid,
    inner: Weak<Inner>,
    changes: watch::Receiver<u64>,
}
impl DemandLease {
    pub(crate) async fn changed(&mut self) {
        let _ = self.changes.changed().await;
    }
    pub(crate) fn snapshot(&self) -> DemandSnapshot {
        let missing = || DemandSnapshot {
            page: None,
            stale: true,
            loading: false,
            failure: Some(PrCode::StaleContext.failure()),
            retention_limited: false,
        };
        let Some(inner) = self.inner.upgrade() else {
            return missing();
        };
        let mut state = inner.state.lock();
        let Some(consumer) = state.consumers.get(&self.id) else {
            return missing();
        };
        let key = consumer.key.clone();
        let seen = consumer.seen;
        if let Some(failure) = &consumer.revoked {
            return DemandSnapshot {
                failure: Some(failure.clone()),
                ..missing()
            };
        }
        let Some(entry) = state.entries.get(&key) else {
            return missing();
        };
        let already_seen = entry
            .page
            .as_ref()
            .is_some_and(|page| Some(page.observation_id) == seen);
        let page = if (entry.authorized && state.auth_result.is_ok()) || already_seen {
            entry.page.clone()
        } else {
            None
        };
        let stale = entry.failure.is_some()
            || state.auth_result.is_err()
            || !entry.authorized
            || !entry.fresh();
        let snapshot = DemandSnapshot {
            page: page.clone(),
            stale,
            loading: entry.job.is_some(),
            failure: entry
                .failure
                .clone()
                .or_else(|| state.auth_result.as_ref().err().cloned()),
            retention_limited: state
                .collections
                .get(&key.collection())
                .is_some_and(|c| c.limited),
        };
        if let Some(page) = page {
            if let Some(consumer) = state.consumers.get_mut(&self.id) {
                consumer.seen = Some(page.observation_id);
            }
            state.used = state.used.saturating_add(1);
            let used = state.used;
            if let Some(entry) = state.entries.get_mut(&key) {
                entry.used = used;
            }
        }
        snapshot
    }
}
impl Drop for DemandLease {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.release(self.id);
        }
    }
}

impl Entry {
    fn fresh(&self) -> bool {
        self.page.as_ref().is_some_and(|page| {
            self.observed.elapsed()
                < if page.confirmed_negative {
                    NEGATIVE_TTL
                } else {
                    POSITIVE_TTL
                }
        })
    }
}
impl State {
    fn live(&self, key: &Key) -> bool {
        self.consumers
            .values()
            .any(|consumer| consumer.key == *key && consumer.visible && consumer.revoked.is_none())
    }
    fn cancel(&mut self, key: &Key) {
        if let Some(entry) = self.entries.get_mut(key) {
            if let Some(job) = entry.job.take() {
                job.abort();
            }
            entry.generation = 0;
        }
    }
    fn remove(&mut self, key: &Key) {
        self.cancel(key);
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
        for consumer in self
            .consumers
            .values_mut()
            .filter(|consumer| consumer.key == *key)
        {
            consumer.revoked = Some(PrCode::StaleContext.failure());
        }
        let collection = key.collection();
        if !self
            .entries
            .keys()
            .any(|key| key.collection() == collection)
        {
            self.collections.remove(&collection);
        }
    }
    fn clear(&mut self) {
        let keys: Vec<_> = self.entries.keys().cloned().collect();
        for key in keys {
            self.remove(&key);
        }
    }
    fn evict_idle(&mut self) {
        let victim = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                entry.job.is_none()
                    && !self
                        .consumers
                        .values()
                        .any(|c| c.key == **key && c.revoked.is_none())
            })
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone());
        if let Some(key) = victim {
            let collection = key.collection();
            self.remove(&key);
            if let Some(group) = self.collections.get_mut(&collection) {
                group.limited = true;
            }
        }
    }
}
impl Inner {
    fn now(&self) -> u64 {
        self.unix_ms
            .saturating_add(self.started.elapsed().as_millis().min(i64::MAX as u128) as u64)
    }
    fn notify(&self) {
        self.changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    fn release(&self, id: Uuid) {
        let mut state = self.state.lock();
        if let Some(consumer) = state.consumers.remove(&id) {
            if !state.live(&consumer.key) {
                state.cancel(&consumer.key);
            }
        }
        self.notify();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/github_coordinator.rs"]
mod unit_tests;

#[cfg(all(test, unix))]
#[path = "../../tests/integration/github_coordinator.rs"]
mod integration_tests;
