//! Exercises PR authority through real host handlers with disposable Git and a controlled native provider.
//! Mock webviews prove IPC admission, not desktop launch, credentials or live provider transport.
use super::*;
use crate::{github::{model::*, service::*, PrIdentity}, workspace::OpenOutcome, test_support};
use std::{collections::VecDeque, sync::{Arc, atomic::{AtomicUsize, Ordering}}, future::Future, pin::Pin, time::Duration};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::{Manager, test::{MockRuntime, mock_builder, get_ipc_response, INVOKE_KEY}};
use tokio::sync::oneshot;

struct Provider {
    account: Mutex<Option<HostAccount>>, calls: AtomicUsize, routes: Mutex<Vec<Option<u64>>>, reviews: Mutex<Vec<PublishedReview>>, comparisons: Mutex<Vec<PublishedComparison>>,
    replies: Mutex<VecDeque<Pin<Box<dyn Future<Output = Publication> + Send>>>>,
}
impl Provider {
    fn new() -> Arc<Self> { Arc::new(Self { account: Mutex::new(Some(HostAccount { host: GithubHost::GithubCom, provider_user_id: "fixture-account".into(), account_epoch: 1 })), calls: AtomicUsize::new(0), routes: Mutex::new(vec![]), reviews: Mutex::new(vec![]), comparisons: Mutex::new(vec![]), replies: Mutex::new(VecDeque::new()) }) }
    fn push(&self, publication: Publication) { self.replies.lock().push_back(Box::pin(async { publication })); }
    fn delay(&self) -> (std::sync::mpsc::Receiver<()>, oneshot::Sender<Publication>) {
        let (entered, receive) = std::sync::mpsc::channel();
        let (send, reply) = oneshot::channel();
        self.replies.lock().push_back(Box::pin(async move { entered.send(()).unwrap(); reply.await.unwrap() }));
        (receive, send)
    }
}
impl PullRequestProvider for Provider {
    fn account(&self) -> Option<HostAccount> { self.account.lock().clone() }
    fn read(&self, request: ProviderRequest) -> Pin<Box<dyn Future<Output = Publication> + Send + '_>> {
        assert_eq!(request.repository.entry_id, request.context.entry_id);
        // The provider receives only the native context and resources admitted for this operation.
        if matches!(request.request, PrRequest::File { .. }) { assert!(request.resources.iter().any(|grant| matches!(grant.resource,Resource::File { .. }))); }
        self.routes.lock().push(request.identity.as_ref().map(|identity| identity.number));
        if let Some(review) = request.review { self.reviews.lock().push(review); }
        if let Some(comparison) = request.comparison { self.comparisons.lock().push(comparison); }
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.replies.lock().pop_front().expect("fixture response must be explicitly queued")
    }
}
struct Fixture { app: tauri::App<MockRuntime>, main: WebviewWindow<MockRuntime>, entry: String, _temp: tempfile::TempDir }
fn fixture(provider: Option<Arc<Provider>>) -> Fixture { fixture_with_inspection(provider,None) }
fn fixture_with_inspection(provider: Option<Arc<Provider>>, executable: Option<&std::path::Path>) -> Fixture {
    let (temp, root) = test_support::working_tree();
    let mut service = RepositoryService::new();
    if let Some(provider) = provider { service.pull_requests = PullRequestService::new(provider); }
    if let Some(executable) = executable { service = service.with_inspection_executable(executable); }
    let entry = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture admission") };
        service.select(&entry_id).await; entry_id
    });
    let app = mock_builder().manage(service).invoke_handler(tauri::generate_handler![
        pr_status, pr_associations, pr_map_head, pr_choose, pr_open, pr_page, pr_refresh, pr_compare,
        pr_files_page, pr_resolve_anchor, pr_file, pr_release, pr_open_link,
    ]).build(super::super::app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    Fixture { app, main, entry, _temp: temp }
}
impl Drop for Fixture { fn drop(&mut self) { tauri::async_runtime::block_on(self.app.state::<RepositoryService>().shutdown()); } }
fn request(window: &WebviewWindow<MockRuntime>, command: &str, body: Value) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest { cmd: command.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1), url: window.url().unwrap(), body: tauri::ipc::InvokeBody::Json(body), headers: Default::default(), invoke_key: INVOKE_KEY.into() }
}
fn response(window: &WebviewWindow<MockRuntime>, command: &str, body: Value) -> Value {
    get_ipc_response(window, request(window, command, body)).unwrap().deserialize().unwrap()
}
fn repository() -> GithubRepository { GithubRepository { id: "fixture-base".into(), host: GithubHost::GithubCom, owner: "fixture".into(), name: "repository".into(), url: "https://github.com/fixture/repository".into() } }
fn collection<T>(items: Vec<T>) -> Collection<T> { Collection { items, total_count: None, next_cursor: None, completeness: Completeness::Complete, limit_reason: None, observed_revision: 999 } }
fn snapshot() -> Publication {
    Publication { result: PrSuccess::Snapshot { snapshot: Snapshot {
        session_id: "untrusted-provider-session".into(), revision: 999, pr_id: "untrusted-provider-pr".into(), observed_at: 7, freshness: Freshness::Fresh, availability: None,
        overview: Overview {
            number: 42, base_repository: repository(), head_repository: None, head_ref: Some("topic".into()), head_oid: Some("b".repeat(40)), base_ref: "main".into(), base_oid: Some("a".repeat(40)),
            title: "Fixture PR".into(), url: "https://github.com/fixture/repository/pull/42".into(), created_at: "2026-01-01T00:00:00Z".into(), updated_at: "2026-01-01T00:00:00Z".into(), closed_at: None, merged_at: None,
            counts: PrCounts { commits: None, files: None, additions: None, deletions: None }, body: Prose::Empty, author: None,
            lifecycle: Lifecycle::Open, draft: false, review_decision: None, reviewers: collection(vec![]), labels: collection(vec![]),
        }, sections: Sections { commits: SectionState::NotLoaded, timeline: SectionState::NotLoaded, threads: SectionState::NotLoaded, thread_comments: SectionState::NotLoaded, reviewers: SectionState::Available, labels: SectionState::Available },
    }}.into(), grants: vec![] }
}
fn open_review(fixture: &Fixture, provider: &Provider) -> Value { open_review_number(fixture,provider,42) }
fn choose_pr(fixture: &Fixture, provider: &Provider, number: u64) -> Value {
    let association = Grant::new(Resource::Association);
    let candidate = Grant::new(Resource::Candidate { association_id: association.id().into(), identity: PrIdentity { host: "github.com".into(), base_repository_id: "fixture-base".into(), number } });
    provider.push(Publication { result: PrSuccess::Association { observation: Association { association_id: association.id().into(), state: AssociationState::Candidates, candidates: vec![Candidate { candidate_id: candidate.id().into(), number, title: "Fixture PR".into(), base_repository: repository() }] }}.into(), grants: vec![association, candidate] });
    let result = response(&fixture.main, "pr_associations", json!({"entryId": fixture.entry, "branch": null}));
    let chosen = response(&fixture.main, "pr_choose", json!({"entryId": fixture.entry, "associationId": result["associationId"], "candidateId": result["candidates"][0]["candidateId"]}));
    chosen
}
fn open_review_number(fixture: &Fixture, provider: &Provider, number: u64) -> Value {
    let chosen = choose_pr(fixture,provider,number);
    let mut observation = snapshot();
    if let PrResult::Success(PrSuccess::Snapshot { snapshot }) = &mut observation.result { snapshot.overview.number = number; }
    provider.push(observation);
    let result = response(&fixture.main, "pr_open", json!({"entryId": fixture.entry, "prId": chosen["prId"]}));
    assert_eq!(result["kind"], "snapshot"); assert_eq!(result["revision"], 0); assert_eq!(result["prId"], chosen["prId"]);
    assert_ne!(result["sessionId"], "untrusted-provider-session"); result
}
fn page_body(f: &Fixture, opened: &Value) -> Value { json!({"entryId": f.entry, "sessionId": opened["sessionId"], "collection": "commits", "cursor": null}) }

#[test]
fn github_host_main_only_commands_reject_companion_and_arbitrary_authority() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone()));
    let companion = tauri::WebviewWindowBuilder::new(&f.app, "companion", Default::default()).build().unwrap();
    let body = json!({"entryId": f.entry, "associationId":"forged", "candidateId":"forged", "prId":"forged", "sessionId":"forged", "comparisonId":"forged", "fileId":"/private/path", "cursor":"query", "linkId":"https://evil.invalid", "anchorId":"forged", "collection":"commits", "selection":{"kind":"aggregate"}, "owner":"fixture", "repository":"repo", "headRef":"topic"});
    for command in ["pr_status", "pr_associations", "pr_map_head", "pr_choose", "pr_open", "pr_page", "pr_refresh", "pr_compare", "pr_files_page", "pr_resolve_anchor", "pr_file", "pr_release", "pr_open_link"] {
        assert!(get_ipc_response(&companion, request(&companion, command, body.clone())).is_err(), "{command}");
    }
    for command in ["pr_choose", "pr_open", "pr_page", "pr_refresh", "pr_compare", "pr_files_page", "pr_resolve_anchor", "pr_file", "pr_release", "pr_open_link"] {
        assert_eq!(response(&f.main, command, body.clone())["kind"], "stale", "{command}");
    }
    let invalid = json!({"entryId":f.entry,"sessionId":"forged","selection":{"kind":"aggregate","path":"/private/path"}});
    assert!(get_ipc_response(&f.main, request(&f.main, "pr_compare", invalid)).is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn github_host_unconfigured_provider_is_truthful_and_local_review_remains_available() {
    let f = fixture(None);
    assert_eq!(response(&f.main, "pr_status", json!({"entryId": f.entry})), json!({"kind":"unavailable","code":"integration_unavailable"}));
    assert_eq!(response(&f.main, "pr_status", json!({"entryId": "/arbitrary/path"}))["code"], "stale_context");
    let local = tauri::async_runtime::block_on(f.app.state::<RepositoryService>().list_contexts(&f.entry));
    assert!(matches!(local, crate::inspection::ContextOptionsResult::Options { .. }));
}
#[test]
fn github_host_rejects_cross_entry_and_account_sessions_without_provider_dispatch() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f, &provider);
    let before = provider.calls.load(Ordering::SeqCst);
    let (_other, root) = test_support::working_tree();
    let second = tauri::async_runtime::block_on(async {
        let service = f.app.state::<RepositoryService>();
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("second admission") };
        service.select(&entry_id).await; entry_id
    });
    let mut body = page_body(&f, &opened); body["entryId"] = second.into();
    assert_eq!(response(&f.main, "pr_page", body)["code"], "stale_context");
    tauri::async_runtime::block_on(f.app.state::<RepositoryService>().select(&f.entry));
    provider.account.lock().as_mut().unwrap().account_epoch = 2;
    assert_eq!(response(&f.main, "pr_page", page_body(&f, &opened))["code"], "stale_context");
    assert_eq!(provider.calls.load(Ordering::SeqCst), before);
}
#[test]
fn github_host_rechecks_account_and_repository_after_delayed_publication() {
    for account_change in [false, true] {
        let provider = Provider::new(); let f = fixture(Some(provider.clone()));
        let (entered, send) = provider.delay(); let main = f.main.clone(); let entry = f.entry.clone();
        let pending = std::thread::spawn(move || response(&main, "pr_status", json!({"entryId":entry})));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        if account_change { provider.account.lock().as_mut().unwrap().account_epoch = 2; }
        else { tauri::async_runtime::block_on(f.app.state::<RepositoryService>().select(&f.entry)); }
        let _ = send.send(Publication { result: PrSuccess::Ready.into(), grants: vec![] });
        assert_eq!(pending.join().unwrap()["code"], "stale_context");
    }
}
#[test]
fn github_host_refresh_rejects_late_old_pages_and_release_cancels_owned_work() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f, &provider);
    let (entered, send) = provider.delay(); let main = f.main.clone(); let body = page_body(&f, &opened);
    let pending = std::thread::spawn(move || response(&main, "pr_page", body));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    provider.push(snapshot());
    let refreshed = response(&f.main, "pr_refresh", json!({"entryId":f.entry,"sessionId":opened["sessionId"]}));
    assert_eq!(refreshed["revision"], 1);
    let _ = send.send(Publication { result: PrSuccess::Page { collection: collection(vec![]) }.into(), grants: vec![] });
    assert_eq!(pending.join().unwrap()["kind"], "stale");
    let (entered, send) = provider.delay(); let main = f.main.clone(); let body = page_body(&f, &opened);
    let pending = std::thread::spawn(move || response(&main, "pr_page", body));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let body = json!({"entryId":f.entry,"sessionId":opened["sessionId"]});
    assert_eq!(response(&f.main, "pr_release", body.clone())["kind"], "released");
    assert_eq!(response(&f.main, "pr_release", body)["kind"], "released");
    assert_eq!(pending.join().unwrap()["kind"], "stale");
    assert!(send.send(PrCode::Network.into()).is_err(), "release must drop provider work");
}
#[test]
fn github_host_failed_refresh_preserves_old_snapshot_authority_and_cursor_scope() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f, &provider);
    let cursor = Grant::new(Resource::Cursor { comparison_id: None, collection: Some(CollectionKind::Commits), thread_id: None, key: "opaque-provider-cursor".into() });
    let cursor_id = cursor.id().to_owned(); let mut page = collection(vec![]); page.next_cursor = Some(cursor_id.clone()); page.completeness = Completeness::More;
    provider.push(Publication { result: PrSuccess::Page { collection: page }.into(), grants: vec![cursor] });
    assert_eq!(response(&f.main,"pr_page",page_body(&f,&opened))["collection"]["observedRevision"],0);
    provider.push(PrCode::Network.into());
    assert_eq!(response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}))["code"],"network");
    let before = provider.calls.load(Ordering::SeqCst); let mut body = page_body(&f,&opened); body["cursor"] = cursor_id.into(); body["collection"] = "labels".into();
    assert_eq!(response(&f.main,"pr_page",body.clone())["code"],"stale_cursor");
    assert_eq!(provider.calls.load(Ordering::SeqCst),before);
    body["collection"] = "commits".into(); provider.push(Publication { result: PrSuccess::Page { collection: collection(vec![]) }.into(), grants: vec![] });
    assert_eq!(response(&f.main,"pr_page",body)["kind"],"page");
}

fn comparison() -> (Publication, String, String, String) {
    let comparison = Grant::new(Resource::Comparison); let comparison_id = comparison.id().to_owned();
    let file = Grant::new(Resource::File { comparison_id: comparison_id.clone(), key: "native-file-key".into() }); let file_id = file.id().to_owned();
    let cursor = Grant::new(Resource::Cursor { comparison_id: Some(comparison_id.clone()), collection: None, thread_id: None, key: "provider-files-cursor".into() }); let cursor_id = cursor.id().to_owned();
    let mut files = collection(vec![PrFile { file_id: file_id.clone(), display_path: "fixture.txt".into(), previous_display_path: None, kind: FileKind::Modified, additions: None, deletions: None, patch: None }]);
    files.next_cursor = Some(cursor_id.clone()); files.completeness = Completeness::More;
    (Publication { result: PrSuccess::Comparison { comparison: Comparison {
        comparison_id: comparison_id.clone(), session_id: "provider".into(), revision: 999, source: ComparisonSource::GithubPatch, scope: ComparisonScope::Aggregate,
        observed_at: 7, observed_head_oid: Some("b".repeat(40)), observed_base_oid: Some("a".repeat(40)), parent_oid: None, base: ComparisonBase { kind: BaseKind::Provider, oid: None }, head_oid: None, files, full_content: false,
    }}.into(), grants: vec![comparison, file, cursor] }, comparison_id, file_id, cursor_id)
}
#[test]
fn github_host_new_comparison_revokes_previous_files_and_supersedes_pending_selection() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f, &provider);
    let body = json!({"entryId":f.entry,"sessionId":opened["sessionId"],"selection":{"kind":"aggregate"}});
    let (first, first_id, file_id, _) = comparison(); provider.push(first);
    assert_eq!(response(&f.main,"pr_compare",body.clone())["comparisonId"],first_id);
    let (entered, send) = provider.delay(); let main = f.main.clone(); let pending_body = body.clone();
    let pending = std::thread::spawn(move || response(&main,"pr_compare",pending_body)); entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let (latest, latest_id, _, _) = comparison(); provider.push(latest);
    assert_eq!(response(&f.main,"pr_compare",body)["comparisonId"],latest_id);
    let (obsolete, _, _, _) = comparison(); let _ = send.send(obsolete);
    assert_eq!(pending.join().unwrap()["kind"],"stale");
    let before = provider.calls.load(Ordering::SeqCst);
    assert_eq!(response(&f.main,"pr_file",json!({"entryId":f.entry,"comparisonId":first_id,"fileId":file_id}))["kind"],"stale");
    assert_eq!(provider.calls.load(Ordering::SeqCst),before);
}

#[test]
fn github_host_entry_removal_and_switch_cancel_pending_reads_without_provider_completion() {
    for remove in [false, true] {
        let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f, &provider);
        let (entered, send) = provider.delay(); let main = f.main.clone(); let body = page_body(&f,&opened);
        let pending = std::thread::spawn(move || response(&main,"pr_page",body)); entered.recv_timeout(Duration::from_secs(5)).unwrap();
        tauri::async_runtime::block_on(async {
            let service = f.app.state::<RepositoryService>();
            if remove { service.remove(&f.entry).await; } else { service.select(&f.entry).await; }
        });
        assert_eq!(pending.join().unwrap()["kind"],"stale");
        assert!(send.send(PrCode::Network.into()).is_err(),"context transition must cancel the provider future");
        let before = provider.calls.load(Ordering::SeqCst);
        assert_eq!(response(&f.main,"pr_page",page_body(&f,&opened))["kind"],"stale");
        assert_eq!(provider.calls.load(Ordering::SeqCst),before);
    }
}

#[test]
fn github_host_native_account_notification_cancels_private_reads_immediately() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f,&provider);
    let (entered, send) = provider.delay(); let main = f.main.clone(); let body = page_body(&f,&opened);
    let pending = std::thread::spawn(move || response(&main,"pr_page",body)); entered.recv_timeout(Duration::from_secs(5)).unwrap();
    provider.account.lock().as_mut().unwrap().account_epoch = 2;
    f.app.state::<RepositoryService>().pull_requests.reconcile_account();
    assert_eq!(pending.join().unwrap()["kind"],"stale");
    assert!(send.send(PrCode::Network.into()).is_err());
}

#[test]
fn github_host_routes_two_sessions_by_native_pr_identity_for_first_pages_refresh_and_aggregate() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone()));
    let first = open_review_number(&f,&provider,42); let second = open_review_number(&f,&provider,43);
    for (opened,number) in [(&first,42),(&second,43)] {
        for (command,body) in [
            ("pr_page",page_body(&f,opened)),
            ("pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]})),
            ("pr_compare",json!({"entryId":f.entry,"sessionId":opened["sessionId"],"selection":{"kind":"aggregate"}})),
        ] {
            provider.push(PrCode::Network.into());
            assert_eq!(response(&f.main,command,body)["code"],"network");
            assert_eq!(*provider.routes.lock().last().unwrap(),Some(number),"{command} must receive native identity");
        }
    }
}
#[test]
fn github_host_wrong_snapshot_identity_cannot_create_or_replace_authority() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let chosen = choose_pr(&f,&provider,42);
    for wrong_repository in [false,true] {
        let mut observation = snapshot();
        if let PrResult::Success(PrSuccess::Snapshot { snapshot }) = &mut observation.result {
            if wrong_repository { snapshot.overview.base_repository.id = "wrong-repository".into(); } else { snapshot.overview.number = 999; }
        }
        provider.push(observation);
        assert_eq!(response(&f.main,"pr_open",json!({"entryId":f.entry,"prId":chosen["prId"]}))["code"],"invalid_output");
    }
    provider.push(snapshot()); let opened = response(&f.main,"pr_open",json!({"entryId":f.entry,"prId":chosen["prId"]}));
    assert_eq!(opened["revision"],0);
    for wrong_repository in [false,true] {
        let mut observation = snapshot();
        if let PrResult::Success(PrSuccess::Snapshot { snapshot }) = &mut observation.result {
            if wrong_repository { snapshot.overview.base_repository.id = "wrong-repository".into(); } else { snapshot.overview.number = 999; }
        }
        provider.push(observation);
        assert_eq!(response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}))["code"],"invalid_output");
        provider.push(Publication { result:PrSuccess::Page { collection:collection(vec![]) }.into(),grants:vec![] });
        assert_eq!(response(&f.main,"pr_page",page_body(&f,&opened))["collection"]["observedRevision"],0);
    }
    provider.push(snapshot());
    assert_eq!(response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}))["revision"],1);
}
#[test]
fn github_host_provider_patch_comparison_cannot_publish_full_content() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f,&provider);
    let (result,comparison_id,file_id,_) = comparison(); provider.push(result);
    assert_eq!(response(&f.main,"pr_compare",json!({"entryId":f.entry,"sessionId":opened["sessionId"],"selection":{"kind":"aggregate"}}))["kind"],"comparison");
    provider.push(Publication { result: PrSuccess::File { comparison_id:comparison_id.clone(),file_id:file_id.clone(),content:FileContent::Text { from_content:"invented old content".into(),to_content:"invented new content".into(),hunks:vec![] } }.into(), grants:vec![] });
    assert_eq!(response(&f.main,"pr_file",json!({"entryId":f.entry,"comparisonId":comparison_id,"fileId":file_id}))["code"],"invalid_output");
}

#[test]
fn github_host_association_branch_validation_preserves_existing_worktree_picker_authority() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone()));
    let root = f._temp.path().join("project"); let linked = f._temp.path().join("linked");
    test_support::git(&root,&["worktree","add","-b","linked",linked.to_str().unwrap()]);
    let worktree = tauri::async_runtime::block_on(async {
        let options = f.app.state::<RepositoryService>().list_contexts(&f.entry).await;
        let crate::inspection::ContextOptionsResult::Options { worktrees,.. } = options else { panic!("fixture choices") };
        worktrees.into_iter().find(|w| w.branch.as_deref()==Some("linked")).unwrap().id
    });
    provider.push(PrCode::Network.into());
    assert_eq!(response(&f.main,"pr_associations",json!({"entryId":f.entry,"branch":"main"}))["code"],"network");
    let selected = tauri::async_runtime::block_on(f.app.state::<RepositoryService>().select_worktree(&f.entry,&worktree));
    assert!(matches!(selected,crate::workspace::MutationOutcome::Updated { .. }),"PR validation must preserve picker worktree IDs");
}

#[test]
fn github_host_continuation_and_file_receive_published_source_endpoints_and_snapshot() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f,&provider);
    let (result,comparison_id,file_id,cursor_id) = comparison(); provider.push(result);
    response(&f.main,"pr_compare",json!({"entryId":f.entry,"sessionId":opened["sessionId"],"selection":{"kind":"aggregate"}}));
    provider.push(Publication { result:PrSuccess::Files { collection:collection(vec![]) }.into(),grants:vec![] });
    assert_eq!(response(&f.main,"pr_files_page",json!({"entryId":f.entry,"comparisonId":comparison_id,"cursor":cursor_id}))["kind"],"files");
    provider.push(Publication { result:PrSuccess::File { comparison_id:comparison_id.clone(),file_id:file_id.clone(),content:FileContent::Patch { patch:ProviderPatch::Unavailable { reason:PrCode::MissingObjects } } }.into(),grants:vec![] });
    assert_eq!(response(&f.main,"pr_file",json!({"entryId":f.entry,"comparisonId":comparison_id,"fileId":file_id}))["kind"],"file");
    let captured = provider.comparisons.lock(); assert_eq!(captured.len(),2);
    for descriptor in captured.iter() {
        assert_eq!(descriptor.comparison_id,comparison_id); assert_eq!(descriptor.identity.number,42);
        assert_eq!(descriptor.review.session.id.to_string(),opened["sessionId"]); assert_eq!(descriptor.review.session.revision,0);
        assert_eq!(descriptor.review.snapshot.base_oid,Some("a".repeat(40))); assert_eq!(descriptor.review.snapshot.head_oid,Some("b".repeat(40)));
        assert!(matches!(descriptor.review.snapshot.lifecycle,Lifecycle::Open)); assert_eq!(descriptor.review.snapshot.observed_at,7);
        assert_eq!(descriptor.review.snapshot.updated_at,"2026-01-01T00:00:00Z");
        assert!(matches!(descriptor.source,ComparisonSource::GithubPatch)); assert!(matches!(descriptor.scope,ComparisonScope::Aggregate));
        assert!(matches!(descriptor.base.kind,BaseKind::Provider)); assert_eq!(descriptor.base.oid,None); assert_eq!(descriptor.head_oid,None); assert!(!descriptor.full_content);
        assert_eq!(descriptor.observed_at,7); assert_eq!(descriptor.observed_head_oid,Some("b".repeat(40))); assert_eq!(descriptor.observed_base_oid,Some("a".repeat(40))); assert_eq!(descriptor.parent_oid,None);
    }
}
#[test]
fn github_host_local_full_comparison_cannot_publish_provider_excerpt() {
    let provider = Provider::new(); let f = fixture(Some(provider.clone())); let opened = open_review(&f,&provider);
    let (mut result,comparison_id,file_id,_) = comparison();
    if let PrResult::Success(PrSuccess::Comparison { comparison }) = &mut result.result {
        comparison.source=ComparisonSource::LocalGit; comparison.full_content=true; comparison.files.next_cursor=None;
        comparison.files.completeness=Completeness::Complete; comparison.base.kind=BaseKind::MergeBase;
        comparison.base.oid=Some("a".repeat(40)); comparison.head_oid=Some("b".repeat(40));
    }
    provider.push(result);
    assert_eq!(response(&f.main,"pr_compare",json!({"entryId":f.entry,"sessionId":opened["sessionId"],"selection":{"kind":"aggregate"}}))["kind"],"comparison");
    provider.push(Publication { result:PrSuccess::File { comparison_id:comparison_id.clone(),file_id:file_id.clone(),content:FileContent::Patch { patch:ProviderPatch::Unavailable { reason:PrCode::MissingObjects } } }.into(),grants:vec![] });
    assert_eq!(response(&f.main,"pr_file",json!({"entryId":f.entry,"comparisonId":comparison_id,"fileId":file_id}))["code"],"invalid_output");
}

#[cfg(unix)]
#[test]
fn github_host_reselection_during_captured_branch_validation_never_dispatches_provider() {
    let gate = tempfile::tempdir().unwrap(); let entered = gate.path().join("entered"); let release = gate.path().join("release");
    let executable = test_support::executable(gate.path(),&format!(
        "case \"$*\" in *for-each-ref*) touch {}; while [ ! -f {} ]; do sleep 0.01; done;; esac\nexec /usr/bin/git \"$@\"",
        test_support::quote(&entered),test_support::quote(&release),
    ));
    let provider = Provider::new(); let f = fixture_with_inspection(Some(provider.clone()),Some(&executable));
    let main=f.main.clone(); let entry=f.entry.clone();
    let pending=std::thread::spawn(move || response(&main,"pr_associations",json!({"entryId":entry,"branch":"main"})));
    let deadline=std::time::Instant::now()+Duration::from_secs(5);
    while !entered.exists() { assert!(std::time::Instant::now()<deadline,"captured ref read must start"); std::thread::sleep(Duration::from_millis(5)); }
    tauri::async_runtime::block_on(f.app.state::<RepositoryService>().select(&f.entry));
    assert_eq!(pending.join().unwrap()["code"],"stale_context");
    assert_eq!(provider.calls.load(Ordering::SeqCst),0);
}

#[test]
fn github_host_deleted_head_preserves_null_repository_and_ref() {
    let provider=Provider::new(); let f=fixture(Some(provider.clone())); let chosen=choose_pr(&f,&provider,42);
    let mut observation=snapshot();
    if let PrResult::Success(PrSuccess::Snapshot { snapshot })=&mut observation.result { snapshot.overview.head_ref=None; snapshot.overview.head_repository=None; }
    provider.push(observation);
    let opened=response(&f.main,"pr_open",json!({"entryId":f.entry,"prId":chosen["prId"]}));
    assert_eq!(opened["kind"],"snapshot"); assert!(opened["overview"]["headRef"].is_null()); assert!(opened["overview"]["headRepository"].is_null());
}
