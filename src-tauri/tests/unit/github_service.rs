//! Exercises native PR admission before provider dispatch.
use super::*;
#[test]
fn github_service_rejects_unknown_session_before_dispatch() {
    let service = PullRequestService::default();
    let context = crate::github::PrContext { entry_id: uuid::Uuid::new_v4().to_string(), repository_generation: 1, account_epoch: 1 };
    let request = PrRequest::Page { session_id: uuid::Uuid::new_v4().to_string(), collection: CollectionKind::Commits, cursor: None, thread_id: None };
    assert_eq!(service.admit(&context, &request), Err(PrCode::StaleContext));
}

fn seeded() -> (PullRequestService, crate::github::PrContext, String, String) {
    let service = PullRequestService::default();
    let context = crate::github::PrContext { entry_id: "fixture-entry".into(), repository_generation: 1, account_epoch: 1 };
    let mut registry = service.registry.lock();
    let mut ids = vec![];
    for number in [1, 2] {
        let session = registry.authority.open(context.clone(), crate::github::PrIdentity { host: "github.com".into(), base_repository_id: "fixture".into(), number }).unwrap();
        let id = session.id.to_string(); ids.push(id.clone());
        registry.sessions.insert(id, Session { snapshot: SnapshotEvidence { base_oid:None,head_oid:None,lifecycle:Lifecycle::Open,updated_at:"fixture".into(),observed_at:0 }, comparison_cancel: watch::channel(false).0, comparison_epoch: 0, context: context.clone(), session, pr_id: "fixture-pr".into(), cancel: watch::channel(false).0 });
    }
    drop(registry);
    (service, context, ids[0].clone(), ids[1].clone())
}
fn issue(service: &PullRequestService, context: &PrContext, session: &str, resource: Resource) -> String {
    let mut registry = service.registry.lock();
    let session = registry.sessions.get(session).unwrap().session.clone();
    let grant = Grant::new(resource); let id = grant.id().to_owned();
    registry.handles.insert(id.clone(), Handle { comparison: None, context: context.clone(), session: Some(session), grant }); id
}
#[test]
fn github_service_binds_files_cursors_and_anchors_to_their_session_and_comparison() {
    let (service, context, first, second) = seeded();
    let comparison = issue(&service,&context,&first,Resource::Comparison);
    let other_comparison = issue(&service,&context,&second,Resource::Comparison);
    let file = issue(&service,&context,&first,Resource::File { comparison_id: comparison.clone(), key:"fixture-file".into() });
    let cursor = issue(&service,&context,&first,Resource::Cursor { comparison_id:Some(comparison.clone()),collection:None,thread_id:None,key:"fixture-cursor".into() });
    assert_eq!(service.admit(&context,&PrRequest::File { comparison_id:comparison.clone(),file_id:file.clone() }),Ok(()));
    assert_eq!(service.admit(&context,&PrRequest::File { comparison_id:other_comparison.clone(),file_id:file }),Err(PrCode::StaleContext));
    assert_eq!(service.admit(&context,&PrRequest::FilesPage { comparison_id:comparison,cursor:cursor.clone() }),Ok(()));
    assert_eq!(service.admit(&context,&PrRequest::FilesPage { comparison_id:other_comparison,cursor }),Err(PrCode::StaleCursor));
    let anchor = issue(&service,&context,&first,Resource::Anchor { key:"fixture-anchor".into() });
    assert_eq!(service.admit(&context,&PrRequest::ResolveAnchor { session_id:first,anchor_id:anchor.clone() }),Ok(()));
    assert_eq!(service.admit(&context,&PrRequest::ResolveAnchor { session_id:second,anchor_id:anchor }),Err(PrCode::StaleContext));
}
#[test]
fn github_service_enforces_thread_cursors_commit_membership_and_parent_bounds() {
    let (service,context,first,second) = seeded();
    let thread = issue(&service,&context,&first,Resource::Thread { key:"fixture-thread".into() });
    let other_thread = issue(&service,&context,&first,Resource::Thread { key:"other-thread".into() });
    let cursor = issue(&service,&context,&first,Resource::Cursor { comparison_id:None,collection:Some(CollectionKind::ThreadComments),thread_id:Some(thread.clone()),key:"cursor".into() });
    let page = |session: &str, thread_id: Option<String>| PrRequest::Page { session_id:session.into(),collection:CollectionKind::ThreadComments,cursor:Some(cursor.clone()),thread_id };
    assert_eq!(service.admit(&context,&page(&first,Some(thread.clone()))),Ok(()));
    assert_eq!(service.admit(&context,&page(&first,Some(other_thread))),Err(PrCode::StaleCursor));
    assert_eq!(service.admit(&context,&page(&first,None)),Err(PrCode::StaleCursor));
    assert!(service.admit(&context,&page(&second,Some(thread))).is_err());
    let commit = issue(&service,&context,&first,Resource::Commit { key:"verified-object".into(),parent_count:2 });
    let compare = |session: &str, parent_index| PrRequest::Compare { session_id:session.into(),selection:ComparisonSelection::Commit { commit_id:commit.clone(),parent_index } };
    assert_eq!(service.admit(&context,&compare(&first,Some(1))),Ok(()));
    assert_eq!(service.admit(&context,&compare(&first,Some(2))),Err(PrCode::StaleContext));
    assert_eq!(service.admit(&context,&compare(&second,None)),Err(PrCode::StaleContext));
}
#[test]
fn github_service_never_uses_a_display_url_as_link_authority() {
    let (service,context,first,_) = seeded();
    let link = issue(&service,&context,&first,Resource::Link { url:"https://github.com/fixture/repository/pull/42".into() });
    assert_eq!(service.admit(&context,&PrRequest::OpenLink { session_id:first.clone(),link_id:link }),Ok(()));
    for raw in ["https://github.com/fixture/repository/pull/42", "javascript:alert(1)", "/private/file"] {
        assert_eq!(service.admit(&context,&PrRequest::OpenLink { session_id:first.clone(),link_id:raw.into() }),Err(PrCode::StaleContext));
    }
    let secret = issue(&service,&context,&first,Resource::Link { url:"https://secret@github.com/fixture/repository/pull/42".into() });
    assert_eq!(service.admit(&context,&PrRequest::OpenLink { session_id:first,link_id:secret }),Err(PrCode::StaleContext));
}
