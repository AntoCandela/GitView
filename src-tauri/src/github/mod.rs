//! Owns GitHub review authority independently of local Git inspection.

mod authority;

pub use authority::{PrAuthority, PrAuthorityError, PrContext, PrIdentity, PrSession};

pub mod model;
pub(crate) mod service;
mod publication;
pub(crate) mod transport;
pub(crate) mod coordinator;
