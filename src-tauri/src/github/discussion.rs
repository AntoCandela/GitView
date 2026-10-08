//! Normalizes bounded provider observations and issues native descriptors without owning sessions.
use super::{model::*, service::Grant, PrIdentity};
use serde_json::Value;
mod fields;
mod overview;
mod page;
mod threads;
mod timeline;

#[derive(Clone, Debug)]
pub(crate) struct PrVersion {
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    pub lifecycle: Lifecycle,
    pub updated_at: String,
}
#[derive(Clone, Debug)]
pub(crate) struct PageAuthority {
    pub identity: PrIdentity,
    pub version: PrVersion,
    pub collection: CollectionKind,
    pub thread_provider_id: Option<String>,
    pub thread_id: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct CursorAuthority {
    pub page: PageAuthority,
    pub cursor: String,
    pub provider_order: u64,
    pub seen_ids: Vec<String>,
    pub pages: u32,
    pub items: u32,
    pub provider_limited: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct ThreadAuthority {
    pub identity: PrIdentity,
    pub version: PrVersion,
    pub provider_id: String,
}
#[derive(Clone, Debug)]
pub(crate) struct AnchorPosition {
    pub commit_oid: String,
    pub path: String,
    pub side: Side,
    pub start_line: Option<u32>,
    pub line: u32,
}
#[derive(Clone, Debug)]
pub(crate) struct AnchorAuthority {
    pub identity: PrIdentity,
    pub version: PrVersion,
    pub thread_provider_id: String,
    pub current: Option<AnchorPosition>,
    pub original: Option<AnchorPosition>,
    pub excerpt: Prose,
    pub url: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct CommitAuthority {
    pub identity: PrIdentity,
    pub version: PrVersion,
    pub oid: String,
    pub parents: Vec<String>,
    pub parents_complete: bool,
}

pub(crate) struct NormalizedOverview {
    pub overview: Overview,
    pub version: PrVersion,
    pub grants: Vec<Grant>,
}
pub(crate) struct NormalizedPage {
    pub collection: Collection<Item>,
    pub grants: Vec<Grant>,
}

/// Accepts the fixed overview envelope for an admitted stable PR identity. Paged metadata
/// remains explicitly limited until the provider composes its independent observations.
pub(crate) fn normalize_overview(
    body: &Value,
    identity: &PrIdentity,
    revision: u64,
) -> Result<NormalizedOverview, Failure> {
    overview::normalize(body, identity, revision)
}
/// Validates one fixed connection response and returns grants for service publication.
/// The caller owns account/session admission and before/after network version checks.
/// Commit pages require full version equality; mutable discussion permits updatedAt drift.
pub(crate) fn normalize_page(
    body: &Value,
    request: &PageAuthority,
    cursor: Option<&CursorAuthority>,
    revision: u64,
) -> Result<NormalizedPage, Failure> {
    page::normalize(body, request, cursor, revision)
}

#[cfg(test)]
#[path = "../../tests/unit/github_discussion.rs"]
mod unit_tests;

const MAX_PROSE: usize = 256 * 1024;
const MAX_ID: usize = 256;
const MAX_ITEMS: usize = 2000;
const MAX_PAGES: u32 = 20;

pub(crate) fn same_version(left: &PrVersion, right: &PrVersion, strict: bool) -> bool {
    left.base_oid == right.base_oid
        && left.head_oid == right.head_oid
        && std::mem::discriminant(&left.lifecycle) == std::mem::discriminant(&right.lifecycle)
        && (!strict || left.updated_at == right.updated_at)
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn oid(value: &str) -> bool {
    [40, 64].contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_identity(identity: &PrIdentity) -> bool {
    identity.host == "github.com"
        && bounded(&identity.base_repository_id, MAX_ID)
        && identity.number > 0
}
impl PrVersion {
    pub(crate) fn validate(&self) -> bool {
        self.base_oid.as_deref().is_none_or(oid)
            && self.head_oid.as_deref().is_none_or(oid)
            && bounded(&self.updated_at, 64)
    }
}
impl PageAuthority {
    pub(crate) fn validate(&self) -> bool {
        valid_identity(&self.identity)
            && self.version.validate()
            && match self.collection {
                CollectionKind::ThreadComments => {
                    self.thread_provider_id
                        .as_deref()
                        .is_some_and(|s| bounded(s, MAX_ID))
                        && self
                            .thread_id
                            .as_deref()
                            .is_some_and(|s| bounded(s, MAX_ID))
                }
                _ => self.thread_provider_id.is_none() && self.thread_id.is_none(),
            }
    }
}
impl CursorAuthority {
    pub(crate) fn validate(&self) -> bool {
        self.page.validate()
            && bounded(&self.cursor, 4096)
            && self.pages > 0
            && self.pages < MAX_PAGES
            && self.items <= MAX_ITEMS as u32
            && self.provider_order <= MAX_ITEMS as u64 + 1
            && self.seen_ids.len() <= MAX_ITEMS
            && self.seen_ids.iter().all(|s| bounded(s, MAX_ID))
    }
}
impl ThreadAuthority {
    pub(crate) fn validate(&self) -> bool {
        valid_identity(&self.identity)
            && self.version.validate()
            && bounded(&self.provider_id, MAX_ID)
    }
}
impl CommitAuthority {
    pub(crate) fn validate(&self) -> bool {
        valid_identity(&self.identity)
            && self.version.validate()
            && oid(&self.oid)
            && self.parents.len() <= 100
            && self.parents.iter().all(|s| oid(s))
    }
}
impl AnchorPosition {
    pub(crate) fn validate(&self) -> bool {
        oid(&self.commit_oid)
            && bounded(&self.path, 4096)
            && !self.path.starts_with('/')
            && !self.path.split('/').any(|s| s == "..")
            && self.line > 0
            && self
                .start_line
                .is_none_or(|start| start > 0 && start <= self.line)
    }
}
impl AnchorAuthority {
    pub(crate) fn validate(&self) -> bool {
        valid_identity(&self.identity)
            && self.version.validate()
            && bounded(&self.thread_provider_id, MAX_ID)
            && (self.current.is_some() || self.original.is_some())
            && self.current.as_ref().is_none_or(AnchorPosition::validate)
            && self.original.as_ref().is_none_or(AnchorPosition::validate)
            && match &self.excerpt {
                Prose::Available { text } | Prose::Limited { text } => text.len() <= MAX_PROSE,
                _ => true,
            }
            && self
                .url
                .as_deref()
                .is_none_or(|url| bounded(url, 4096) && url.starts_with("https://github.com/"))
    }
}
