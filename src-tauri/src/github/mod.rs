//! Owns GitHub review authority independently of local Git inspection.

mod authority;

pub use authority::{PrAuthority, PrAuthorityError, PrContext, PrIdentity, PrSession};
