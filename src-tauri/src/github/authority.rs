//! Binds PR reads to native repository, account and published snapshot generations.

use std::collections::HashMap;
use uuid::Uuid;

/// Native-observed context. Neither account identity nor generation is renderer authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrContext {
    pub entry_id: Uuid,
    pub repository_generation: u64,
    pub account_epoch: u64,
}

/// Provider identity uses the base repository's stable ID, not its mutable name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrIdentity {
    pub host: String,
    pub base_repository_id: String,
    pub number: u64,
}

/// Opaque authority issued after native association and account verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrSession {
    pub id: Uuid,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrAuthorityError {
    StaleContext,
    ResourceLimit,
}

struct Review {
    context: PrContext,
    identity: PrIdentity,
    revision: u64,
}

/// Bounded native session registry. Callers synchronize access and perform I/O outside its lock.
///
/// Every read must validate both before dispatch and before publication using a freshly
/// observed native context. Admission alone does not authorize a late completion.
pub struct PrAuthority {
    reviews: HashMap<Uuid, Review>,
    capacity: usize,
}

impl PrAuthority {
    pub fn new(capacity: usize) -> Self {
        Self { reviews: HashMap::new(), capacity }
    }

    /// Creates authority for a verified association without changing the Git repository.
    pub fn open(&mut self, context: PrContext, identity: PrIdentity) -> Result<PrSession, PrAuthorityError> {
        if self.reviews.len() >= self.capacity {
            return Err(PrAuthorityError::ResourceLimit);
        }
        let id = Uuid::new_v4();
        self.reviews.insert(id, Review { context, identity, revision: 0 });
        Ok(PrSession { id, revision: 0 })
    }

    /// Rejects unknown, released or superseded sessions without disclosing their identity.
    pub fn validate(&self, context: &PrContext, session: &PrSession) -> Result<&PrIdentity, PrAuthorityError> {
        let review = self.reviews.get(&session.id).ok_or(PrAuthorityError::StaleContext)?;
        if review.context != *context || review.revision != session.revision {
            return Err(PrAuthorityError::StaleContext);
        }
        Ok(&review.identity)
    }

    /// Adopts a replacement provider observation and revokes all older read authority.
    /// Call only after validating the complete replacement; a failed refresh retains the old revision.
    pub fn replace_snapshot(&mut self, context: &PrContext, session: &PrSession) -> Result<PrSession, PrAuthorityError> {
        self.validate(context, session)?;
        let next = session.revision.checked_add(1).ok_or(PrAuthorityError::ResourceLimit)?;
        self.reviews.get_mut(&session.id).ok_or(PrAuthorityError::StaleContext)?.revision = next;
        Ok(PrSession { id: session.id, revision: next })
    }

    /// Revokes a review only within its native owner context; stale revisions may still release it.
    pub fn release(&mut self, context: &PrContext, id: Uuid) -> Result<(), PrAuthorityError> {
        let review = self.reviews.get(&id).ok_or(PrAuthorityError::StaleContext)?;
        if review.context != *context { return Err(PrAuthorityError::StaleContext); }
        self.reviews.remove(&id);
        Ok(())
    }

    /// Removes authority when the entry is removed or its repository/configuration changes.
    pub fn invalidate_entry(&mut self, entry_id: Uuid) {
        self.reviews.retain(|_, review| review.context.entry_id != entry_id);
    }

    /// Clears all private review authority when the effective account becomes unknown or changes.
    pub fn invalidate_account(&mut self) {
        self.reviews.clear();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/github_authority.rs"]
mod unit_tests;
