//! Exercises session isolation and late-result revocation without provider or user data.

use super::*;

fn context() -> PrContext {
    PrContext { entry_id: Uuid::new_v4(), repository_generation: 1, account_epoch: 1 }
}

fn identity(number: u64) -> PrIdentity {
    PrIdentity { host: "github.com".into(), base_repository_id: "fixture-repository".into(), number }
}

#[test]
fn github_authority_rejects_cross_entry_account_and_generation_reads() {
    let mut registry = PrAuthority::new(4);
    let owner = context();
    let session = registry.open(owner.clone(), identity(42)).unwrap();
    assert_eq!(registry.validate(&owner, &session), Ok(&identity(42)));
    let others = [
        PrContext { entry_id: Uuid::new_v4(), ..owner.clone() },
        PrContext { account_epoch: 2, ..owner.clone() },
        PrContext { repository_generation: 2, ..owner.clone() },
    ];
    for other in others {
        assert_eq!(registry.validate(&other, &session), Err(PrAuthorityError::StaleContext));
        assert_eq!(registry.release(&other, session.id), Err(PrAuthorityError::StaleContext));
    }
    assert_eq!(registry.validate(&owner, &session), Ok(&identity(42)));
}

#[test]
fn github_authority_rechecks_pending_reads_after_snapshot_replacement() {
    let mut registry = PrAuthority::new(2);
    let owner = context();
    let pending = registry.open(owner.clone(), identity(42)).unwrap();
    registry.validate(&owner, &pending).unwrap();
    let current = registry.replace_snapshot(&owner, &pending).unwrap();
    assert_eq!(registry.validate(&owner, &pending), Err(PrAuthorityError::StaleContext));
    assert_eq!(registry.replace_snapshot(&owner, &pending), Err(PrAuthorityError::StaleContext));
    assert_eq!(registry.validate(&owner, &current), Ok(&identity(42)));
    registry.release(&owner, pending.id).unwrap();
    assert_eq!(registry.validate(&owner, &current), Err(PrAuthorityError::StaleContext));
}

#[test]
fn github_authority_invalidates_only_the_changed_entry() {
    let mut registry = PrAuthority::new(2);
    let first = context();
    let second = context();
    let first_session = registry.open(first.clone(), identity(42)).unwrap();
    let second_session = registry.open(second.clone(), identity(43)).unwrap();
    registry.invalidate_entry(first.entry_id);
    assert_eq!(registry.validate(&first, &first_session), Err(PrAuthorityError::StaleContext));
    assert_eq!(registry.validate(&second, &second_session), Ok(&identity(43)));
    registry.invalidate_account();
    assert_eq!(registry.validate(&second, &second_session), Err(PrAuthorityError::StaleContext));
}

#[test]
fn github_authority_bounds_retention_and_never_reuses_released_handles() {
    let mut registry = PrAuthority::new(1);
    let owner = context();
    let old = registry.open(owner.clone(), identity(42)).unwrap();
    assert_eq!(registry.open(owner.clone(), identity(43)), Err(PrAuthorityError::ResourceLimit));
    assert_eq!(registry.validate(&owner, &old), Ok(&identity(42)));
    registry.release(&owner, old.id).unwrap();
    let new = registry.open(owner.clone(), identity(43)).unwrap();
    assert_ne!(old.id, new.id);
    assert_eq!(registry.validate(&owner, &old), Err(PrAuthorityError::StaleContext));
    assert_eq!(registry.validate(&owner, &new), Ok(&identity(43)));
    assert_eq!(PrAuthority::new(0).open(owner, identity(42)), Err(PrAuthorityError::ResourceLimit));
}
