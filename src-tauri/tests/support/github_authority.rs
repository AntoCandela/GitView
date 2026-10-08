//! Supplies valid native-only review descriptors for service authority fixtures.
use crate::github::{discussion::*, model::*, service::CursorTarget, PrIdentity};
pub(crate) fn identity() -> PrIdentity { PrIdentity { host:"github.com".into(),base_repository_id:"fixture-base".into(),number:42 } }
pub(crate) fn repository() -> GithubRepository { GithubRepository { id:"fixture-base".into(),host:GithubHost::GithubCom,owner:"fixture".into(),name:"repository".into(),url:"https://github.com/fixture/repository".into() } }
pub(crate) fn version() -> PrVersion { PrVersion { base_oid:Some("a".repeat(40)),head_oid:Some("b".repeat(40)),lifecycle:Lifecycle::Open,updated_at:"2026-01-01T00:00:00Z".into() } }
pub(crate) fn cursor(collection:CollectionKind, thread_id:Option<String>) -> CursorTarget {
    CursorTarget::Discussion(Box::new(CursorAuthority { page:PageAuthority { identity:identity(),version:version(),collection,thread_provider_id:thread_id.as_ref().map(|_|"fixture-thread".into()),thread_id },cursor:"fixture-cursor".into(),provider_order:1,provider_limited:false,seen_ids:vec![],pages:1,items:0 }))
}
pub(crate) fn thread(id:&str) -> Box<ThreadAuthority> { Box::new(ThreadAuthority { identity:identity(),version:version(),provider_id:id.into() }) }
pub(crate) fn anchor() -> Box<AnchorAuthority> { Box::new(AnchorAuthority { identity:identity(),version:version(),thread_provider_id:"fixture-thread".into(),current:Some(AnchorPosition { commit_oid:"b".repeat(40),path:"fixture.txt".into(),side:Side::New,start_line:None,line:1 }),original:None,excerpt:Prose::Empty,url:None }) }
pub(crate) fn commit() -> Box<CommitAuthority> { Box::new(CommitAuthority { identity:identity(),version:version(),oid:"b".repeat(40),parents:vec!["a".repeat(40),"c".repeat(40)],parents_complete:true }) }
