//! Owns shared account observations, bounded retries and cancellable collection work.
use super::*;
const RETRIES: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];
// Longer guidance remains visible and enforced, without retaining a multi-day automatic timer.
const MAX_AUTOMATIC_WAIT: Duration = Duration::from_secs(86400);

impl Inner {
    fn cooldown(&self, state: &State) -> Option<Failure> {
        state
            .cooldown
            .filter(|until| *until > self.now())
            .map(|until| Failure {
                retry_at: Some(until),
                ..PrCode::RateLimited.failure()
            })
    }
    fn remember_throttle(&self, state: &mut State, failure: &Failure) {
        if failure.code == PrCode::RateLimited {
            if let Some(until) = failure.retry_at {
                state.cooldown = Some(state.cooldown.unwrap_or(0).max(until));
            }
        }
    }
    pub(super) fn revoke_account(&self, state: &mut State, failure: Failure) {
        state.epoch = state.epoch.wrapping_add(1);
        state.account = None;
        state.auth_revision = state.auth_revision.wrapping_add(1);
        state.auth_result = Err(failure.clone());
        state.clear();
        for consumer in state.consumers.values_mut() {
            consumer.revoked = Some(failure.clone());
        }
        self.accounts.send_replace(AccountNotice {
            account: None,
            authorization_known: false,
        });
        self.notify();
    }
    pub(super) async fn observe(&self) -> Result<HostAccount, Failure> {
        let revision = self.state.lock().auth_revision;
        let _gate = self.auth_gate.lock().await;
        {
            let state = self.state.lock();
            if state.closed {
                return Err(PrCode::StaleContext.failure());
            }
            if let Some(failure) = self.cooldown(&state) {
                return Err(failure);
            }
            if state.auth_revision != revision {
                return state.auth_result.clone();
            }
        }
        let result = self.transport.observe().await;
        let mut state = self.state.lock();
        if state.auth_revision != revision || state.closed {
            return Err(PrCode::StaleContext.failure());
        }
        match result {
            Ok(observation) => {
                if state
                    .account
                    .as_ref()
                    .is_none_or(|a| a.provider_user_id != observation.provider_user_id)
                {
                    state.epoch = state.epoch.wrapping_add(1);
                    state.clear();
                    state.account = Some(HostAccount {
                        host: GithubHost::GithubCom,
                        provider_user_id: observation.provider_user_id,
                        account_epoch: state.epoch,
                    });
                    self.notify();
                }
                let account = state
                    .account
                    .clone()
                    .ok_or_else(|| PrCode::AuthUnavailable.failure())?;
                state.auth_revision = state.auth_revision.wrapping_add(1);
                state.auth_result = Ok(account.clone());
                self.accounts.send_replace(AccountNotice {
                    account: Some(account.clone()),
                    authorization_known: true,
                });
                self.notify();
                Ok(account)
            }
            Err(failure) => {
                self.remember_throttle(&mut state, &failure);
                if revokes(failure.code) {
                    self.revoke_account(&mut state, failure.clone());
                } else {
                    state.auth_revision = state.auth_revision.wrapping_add(1);
                    state.auth_result = Err(failure.clone());
                    self.accounts.send_replace(AccountNotice {
                        account: state.account.clone(),
                        authorization_known: false,
                    });
                    self.notify();
                }
                Err(failure)
            }
        }
    }
    pub(super) fn launch(self: &Arc<Self>, state: &mut State, key: &Key, force: bool) {
        if !state.live(key) {
            return;
        }
        let Some(entry) = state.entries.get_mut(key) else {
            return;
        };
        if entry.job.is_some() {
            entry.force_read |= force;
            return;
        }
        entry.force_read = force;
        let Some(generation) = state.next_job.checked_add(1) else {
            entry.failure = Some(PrCode::ResourceLimit.failure());
            return;
        };
        state.next_job = generation;
        entry.generation = generation;
        entry.authorized = false;
        entry.failure = None;
        let inner = self.clone();
        let key = key.clone();
        let context =
            crate::diagnostics::OperationContext::current().map(|context| context.child());
        let completion = JobCompletion {
            inner: Arc::downgrade(self),
            key: key.clone(),
            generation,
        };
        let work = crate::native_work::inherit(async move {
            match context {
                Some(context) => context.scope(inner.drive(key, generation, force)).await,
                None => inner.drive(key, generation, force).await,
            }
        });
        let task = tokio::spawn(async move {
            let _completion = completion;
            work.await;
        });
        entry.job = Some(task.abort_handle());
        self.notify();
    }
    fn valid(&self, state: &State, key: &Key, generation: u64) -> bool {
        state.live(key)
            && state
                .entries
                .get(key)
                .is_some_and(|entry| entry.generation == generation)
            && state
                .account
                .as_ref()
                .is_some_and(|a| a.account_epoch == key.epoch && a.provider_user_id == key.user)
    }
    async fn drive(self: Arc<Self>, key: Key, generation: u64, force: bool) {
        let gate = {
            let state = self.state.lock();
            let Some(group) = state.collections.get(&key.collection()) else {
                return;
            };
            group.gate.clone()
        };
        let Ok(_permit) = gate.acquire_owned().await else {
            return;
        };
        for attempt in 0..=RETRIES.len() {
            let cooldown = {
                let state = self.state.lock();
                if !self.valid(&state, &key, generation) {
                    return;
                }
                self.cooldown(&state)
            };
            if let Some(failure) = cooldown {
                self.failure(&key, generation, failure.clone());
                if !self.wait_guidance(&failure).await {
                    break;
                }
            }
            match self.attempt(&key, generation, force).await {
                Ok(()) => break,
                Err(failure) => {
                    self.failure(&key, generation, failure.clone());
                    if attempt == RETRIES.len() {
                        break;
                    }
                    if failure.code == PrCode::RateLimited {
                        if !self.wait_guidance(&failure).await {
                            break;
                        }
                    } else if matches!(failure.code, PrCode::Network | PrCode::Timeout) {
                        tokio::time::sleep(RETRIES[attempt]).await;
                    } else {
                        break;
                    }
                }
            }
        }
        let mut state = self.state.lock();
        if let Some(entry) = state
            .entries
            .get_mut(&key)
            .filter(|e| e.generation == generation)
        {
            entry.job = None;
        }
        self.notify();
    }
    async fn wait_guidance(&self, failure: &Failure) -> bool {
        let Some(until) = failure.retry_at else {
            return false;
        };
        let delay = Duration::from_millis(until.saturating_sub(self.now()));
        if delay > MAX_AUTOMATIC_WAIT {
            return false;
        }
        tokio::time::sleep(delay.max(Duration::from_millis(1))).await;
        true
    }
    async fn attempt(&self, key: &Key, generation: u64, force: bool) -> Result<(), Failure> {
        let before = self.observe().await?;
        {
            let mut state = self.state.lock();
            if !self.valid(&state, key, generation) || before.account_epoch != key.epoch {
                return Err(PrCode::StaleContext.failure());
            }
            if !force
                && state
                    .entries
                    .get(key)
                    .is_some_and(|entry| !entry.force_read && entry.fresh())
            {
                if let Some(entry) = state.entries.get_mut(key) {
                    entry.authorized = true;
                    entry.failure = None;
                    // Complete the cache-only job atomically with its decision. A Refresh after
                    // this point starts a forced job rather than joining an already-decided hit.
                    entry.job = None;
                }
                self.notify();
                return Ok(());
            }
        }
        let response = self.transport.read(key.read.clone()).await?;
        if response.rate.remaining == Some(0) {
            if let Some(retry_at) = response.rate.retry_at {
                return Err(Failure {
                    retry_at: Some(retry_at),
                    ..PrCode::RateLimited.failure()
                });
            }
        }
        let after = self.observe().await?;
        if before != after {
            return Err(PrCode::StaleContext.failure());
        }
        let page = cache::decode(&key.read, response, self.now())?;
        let mut state = self.state.lock();
        if !self.valid(&state, key, generation) {
            return Err(PrCode::StaleContext.failure());
        }
        cache::retain(&mut state, key, page)?;
        self.notify();
        Ok(())
    }
    fn failure(&self, key: &Key, generation: u64, failure: Failure) {
        let mut state = self.state.lock();
        if !self.valid(&state, key, generation) {
            return;
        }
        self.remember_throttle(&mut state, &failure);
        if revokes(failure.code) {
            self.revoke_account(&mut state, failure);
            return;
        }
        if failure.code == PrCode::RepositoryUnavailable {
            let scope = key.scope.common_storage.clone();
            let identity = key.scope.identity.clone();
            let keys: Vec<_> = state
                .entries
                .keys()
                .filter(|k| {
                    k.scope.common_storage == scope
                        && (identity.is_none() || k.scope.identity == identity)
                })
                .cloned()
                .collect();
            for key in keys {
                state.remove(&key);
                for consumer in state.consumers.values_mut().filter(|c| c.key == key) {
                    consumer.revoked = Some(failure.clone());
                }
            }
        } else if let Some(entry) = state.entries.get_mut(key) {
            entry.failure = Some(failure);
            entry.authorized = false;
        }
        self.notify();
    }
}
fn revokes(code: PrCode) -> bool {
    matches!(
        code,
        PrCode::AuthRequired | PrCode::AuthUnavailable | PrCode::AccessDenied
    )
}

struct JobCompletion {
    inner: Weak<Inner>,
    key: Key,
    generation: u64,
}
impl Drop for JobCompletion {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            let mut state = inner.state.lock();
            if let Some(entry) = state
                .entries
                .get_mut(&self.key)
                .filter(|entry| entry.generation == self.generation && entry.job.is_some())
            {
                entry.job = None;
                entry.authorized = false;
                entry.failure = Some(PrCode::StaleContext.failure());
                inner.notify();
            }
        }
    }
}
