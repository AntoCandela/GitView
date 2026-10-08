//! Exercises review reads across mock IPC, disposable Git, and an isolated fake gh process.
//! These fixtures do not certify desktop launch or live GitHub authentication.
use super::*;

fn review_fixture() -> (Fixture, tempfile::TempDir) {
    let gh = tempfile::tempdir().unwrap();
    std::fs::write(gh.path().join("provider.py"), PROVIDER).unwrap();
    let executable = test_support::executable(gh.path(), "exec python3 \"$FIXTURE_ROOT/provider.py\" \"$@\"");
    let adapter = crate::github::transport::GhReadAdapter::fixture(&executable, vec![
        ("FIXTURE_ROOT".into(), gh.path().as_os_str().into()),
        ("GH_CONFIG_DIR".into(), gh.path().join("config").into_os_string()),
        ("GH_TOKEN".into(), "fixture-token".into()),
    ]);
    let coordinator = Arc::new(crate::github::coordinator::PrDemandCoordinator::new(adapter));
    let provider = crate::github::provider::GithubProvider::new(coordinator);
    let f = fixture_with_provider(Some(Arc::new(provider)), None);
    let root = tauri::async_runtime::block_on(f.app.state::<RepositoryService>().with_pull_request_context(&f.entry, |native| native.unwrap().0.root));
    test_support::git(&root, &["remote", "add", "origin", "git@work-alias:fork/project.git"]);
    (f, gh)
}
fn open_concrete(f: &Fixture) -> Value {
    let mapped = mapped_association(f);
    let chosen = response(&f.main,"pr_choose",json!({"entryId":f.entry,"associationId":mapped["associationId"],"candidateId":mapped["candidates"][0]["candidateId"]}));
    let opened = response(&f.main,"pr_open",json!({"entryId":f.entry,"prId":chosen["prId"]}));
    assert_eq!(opened["kind"],"snapshot","{opened}");
    opened
}
fn read_page(f: &Fixture, opened: &Value, kind: &str, cursor: Value, thread: Value) -> Value {
    response(&f.main,"pr_page",json!({"entryId":f.entry,"sessionId":opened["sessionId"],"collection":kind,"cursor":cursor,"threadId":thread}))
}
fn set_mode(gh: &tempfile::TempDir, mode: &str) { std::fs::write(gh.path().join(mode),"").unwrap(); }

#[test]
fn github_review_host_optional_label_failure_preserves_description_and_reviewers() {
    let (f,gh)=review_fixture();set_mode(&gh,"labels-fail");
    let opened=open_concrete(&f);
    assert_eq!(opened["overview"]["title"],"Fixture review");
    assert_eq!(opened["overview"]["body"]["text"],"Review **description**");
    assert_eq!(opened["sections"]["reviewers"],"available");
    assert_eq!(opened["overview"]["reviewers"]["items"].as_array().unwrap().len(),1);
    assert_eq!(opened["sections"]["labels"],"unavailable");
    assert!(opened["availability"].is_object());
}

#[test]
fn github_review_host_nested_comments_require_their_own_cursor_and_thread() {
    let (f,_gh)=review_fixture();let opened=open_concrete(&f);
    let timeline=read_page(&f,&opened,"timeline",Value::Null,Value::Null);
    let threads=read_page(&f,&opened,"threads",Value::Null,Value::Null);
    assert_eq!(timeline["kind"],"page","{timeline}");assert_eq!(threads["kind"],"page","{threads}");
    let thread=&threads["collection"]["items"][0]["thread"];
    let own_cursor=thread["comments"]["nextCursor"].clone();assert!(own_cursor.is_string(),"{threads}");
    let wrong=read_page(&f,&opened,"thread_comments",timeline["collection"]["nextCursor"].clone(),thread["id"].clone());
    assert_eq!(wrong["code"],"stale_cursor","{wrong}");
    let next=read_page(&f,&opened,"thread_comments",own_cursor,thread["id"].clone());
    assert_eq!(next["kind"],"page","{next}");
    assert_eq!(next["collection"]["items"].as_array().unwrap().len(),1);
}

#[test]
fn github_review_host_force_push_rejects_membership_then_refresh_revokes_cursors() {
    let (f,gh)=review_fixture();let opened=open_concrete(&f);
    let first=read_page(&f,&opened,"commits",Value::Null,Value::Null);
    assert_eq!(first["kind"],"page","{first}");
    let cursor=first["collection"]["nextCursor"].clone();assert!(cursor.is_string());
    set_mode(&gh,"changed");
    let next=read_page(&f,&opened,"commits",cursor.clone(),Value::Null);
    assert_eq!(next["code"],"changed_snapshot","{next}");
    assert_eq!(opened["overview"]["headOid"],"b".repeat(40));
    let refreshed=response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}));
    assert_eq!(refreshed["kind"],"snapshot","{refreshed}");
    assert_eq!(refreshed["overview"]["title"],"Updated review");
    assert_eq!(refreshed["overview"]["headOid"],"c".repeat(40));
    assert_eq!(refreshed["revision"],1);
    assert_eq!(read_page(&f,&refreshed,"commits",cursor,Value::Null)["code"],"stale_cursor");
}

#[test]
fn github_review_host_identity_and_partial_activity_are_not_successful_complete_history() {
    let (f,gh)=review_fixture();let opened=open_concrete(&f);
    set_mode(&gh,"unknown");
    let page=read_page(&f,&opened,"timeline",Value::Null,Value::Null);
    assert_eq!(page["kind"],"page","{page}");
    assert_eq!(page["collection"]["completeness"],"limited","{page}");
    set_mode(&gh,"wrong-id");
    let refresh=response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}));
    assert_eq!(refresh["code"],"invalid_output","{refresh}");
    std::fs::remove_file(gh.path().join("wrong-id")).unwrap();set_mode(&gh,"partial");
    let threads=read_page(&f,&opened,"threads",Value::Null,Value::Null);
    assert_ne!(threads["collection"]["completeness"],"complete","{threads}");
    assert_eq!(threads["code"],"invalid_output","{threads}");
    assert_eq!(opened["overview"]["title"],"Fixture review");
}

#[test]
fn github_review_host_trailing_observation_rejects_head_change_during_commit_batch() {
    let (f,gh)=review_fixture();let opened=open_concrete(&f);
    set_mode(&gh,"change-after-commits");
    let page=read_page(&f,&opened,"commits",Value::Null,Value::Null);
    assert_eq!(page["code"],"changed_snapshot","{page}");
    assert!(page.get("collection").is_none(),"No new commit handles may be published: {page}");
    assert!(gh.path().join("changed").exists(),"The fake provider must exercise the post-page race");
    let refreshed=response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}));
    assert_eq!(refreshed["kind"],"snapshot","The old session remains admitted for refresh: {refreshed}");
    assert_eq!(refreshed["revision"],1);
}

#[test]
fn github_review_host_partial_description_preserves_core_metadata() {
    let (f,gh)=review_fixture();set_mode(&gh,"body-partial");
    let opened=open_concrete(&f);
    assert_eq!(opened["overview"]["title"],"Fixture review");
    assert_eq!(opened["overview"]["body"]["kind"],"unavailable","{opened}");
    assert_eq!(opened["overview"]["body"]["code"],"invalid_output");
}

#[test]
fn github_review_host_partial_nested_comments_preserve_thread_without_authority() {
    let (f,gh)=review_fixture();let opened=open_concrete(&f);set_mode(&gh,"comments-partial");
    let page=read_page(&f,&opened,"threads",Value::Null,Value::Null);
    assert_eq!(page["kind"],"page","{page}");
    let thread=&page["collection"]["items"][0]["thread"];
    assert_eq!(thread["path"],"file.txt","{page}");
    assert_eq!(thread["resolved"],false);
    assert_eq!(thread["comments"]["completeness"],"limited");
    assert_eq!(thread["comments"]["limitReason"],"provider");
    assert!(thread["comments"]["nextCursor"].is_null());
    assert!(thread["anchorId"].is_null());
}

#[test]
fn github_review_host_partial_description_with_auth_error_fails_closed() {
    let (f,gh)=review_fixture();let opened=open_concrete(&f);
    set_mode(&gh,"body-partial");set_mode(&gh,"body-auth");
    let failed=response(&f.main,"pr_refresh",json!({"entryId":f.entry,"sessionId":opened["sessionId"]}));
    assert!(failed["code"]=="auth_required" || failed["code"]=="stale_context","{failed}");
    assert_ne!(failed["kind"],"snapshot");
    assert!(failed.get("overview").is_none());
    assert!(failed.get("sessionId").is_none());
}

const PROVIDER: &str = r#"
import json,os,sys
from pathlib import Path
root=Path(os.environ['FIXTURE_ROOT'])
def mode(name): return (root/name).exists()
def emit(body): print('HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n'+json.dumps(body),end='')
args=sys.argv[1:]
if args[0]=='version': print('gh version 2.81.0');sys.exit()
if args[0]=='auth': print(json.dumps({'hosts':{'github.com':[{'host':'github.com','active':True,'state':'success'}]}}));sys.exit()
repo={'id':2,'name':'project','owner':{'login':'fork'},'fork':False}
pr={'number':7,'title':'Fixture review','state':'open','merged_at':None,'base':{'ref':'main','repo':repo},'head':{'ref':'published','repo':repo}}
if args[1]!='graphql':
    emit(repo if args[1]=='repos/fork/project' else pr if args[1].endswith('/7') else [pr]);sys.exit()
payload=json.load(sys.stdin);q=payload['query'];v=payload['variables']
if 'GitViewViewer' in q: emit({'data':{'viewer':{'id':'fixture-viewer'}}});sys.exit()
time='2026-01-01T00:00:00Z';actor={'id':'user-one','login':'fixture','name':'Fixture'}
r={'databaseId':99 if mode('wrong-id') else 2,'name':'project','nameWithOwner':'fork/project','owner':{'login':'fork'},'url':'https://github.com/fork/project'}
p={'number':7,'title':'Updated review' if mode('changed') else 'Fixture review','body':'Review **description**','state':'OPEN','isDraft':False,'mergedAt':None,'closedAt':None,'createdAt':time,'updatedAt':time,'url':'https://github.com/fork/project/pull/7','author':actor,'headRefName':'published','baseRefName':'main','headRefOid':('c' if mode('changed') else 'b')*40,'baseRefOid':'a'*40,'repository':r,'headRepository':r,'reviewDecision':'REVIEW_REQUIRED','changedFiles':1,'additions':2,'deletions':1,'commits':{'totalCount':2}}
def connection(nodes,more=False,cursor=None): return {'nodes':nodes,'totalCount':len(nodes)+(1 if more else 0),'pageInfo':{'hasNextPage':more,'endCursor':cursor}}
def comment(identifier,reply=None): return {'id':identifier,'body':'Comment body','createdAt':time,'updatedAt':time,'url':'https://github.com/fork/project/pull/7#discussion_r1','author':actor,'replyTo':None if reply is None else {'id':reply},'originalCommit':{'oid':'b'*40},'commit':{'oid':'b'*40},'diffHunk':'@@ -1 +1 @@\n-old\n+new','path':'file.txt','line':1,'startLine':None,'originalLine':1,'originalStartLine':None,'outdated':False,'pullRequestReview':{'id':'review-one'}}
thread={'id':'thread-one','path':'file.txt','line':1,'startLine':None,'originalLine':1,'originalStartLine':None,'diffSide':'RIGHT','startDiffSide':None,'isResolved':False,'isOutdated':False,'subjectType':'LINE','comments':connection([comment('comment-one')],True,'comment-next')}
if 'GitViewOverview' in q: pass
elif 'GitViewThread(' in q:
    thread['pullRequest']=p;thread['comments']=connection([comment('comment-two','comment-one')]);emit({'data':{'node':thread}});sys.exit()
elif 'reviewRequests(first:' in q: p['reviewRequests']=connection([{'id':'request-one','requestedReviewer':dict(actor,__typename='User')}])
elif 'labels(first:' in q:
    if mode('labels-fail'): emit({'errors':[{'message':'fixture unavailable'}]});sys.exit()
    p['labels']=connection([])
elif 'timelineItems(first:' in q: p['timelineItems']=connection([{'__typename':'FutureEvent'}] if mode('unknown') else [{'__typename':'IssueComment','id':'issue-one','body':'Discussion','createdAt':time,'updatedAt':time,'url':p['url']+'#issuecomment-1','author':actor}],not mode('unknown'),'timeline-next' if not mode('unknown') else None)
elif 'reviewThreads(first:' in q: p['reviewThreads']=connection([thread])
elif 'commits(first:' in q: p['commits']=connection([{'id':'commit-one','commit':{'oid':'b'*40,'message':'Change','authoredDate':time,'committedDate':time,'url':'https://github.com/fork/project/commit/'+'b'*40,'parents':connection([{'oid':'a'*40}])}}],v.get('cursor') is None,'commit-next' if v.get('cursor') is None else None)
# Mutate only after constructing the old-version commit response.
if 'commits(first:' in q and mode('change-after-commits'): (root/'changed').touch()
result={'data':{'repository':dict(r,pullRequest=p)}}
if mode('body-partial') and 'GitViewOverview' in q:
    p['body']=None
    result['errors']=[{'message':'fixture body unavailable','path':['repository','pullRequest','body']}]
    if mode('body-auth'): result['errors'].append({'type':'UNAUTHENTICATED','message':'fixture auth failure'})
if mode('comments-partial') and 'reviewThreads(first:' in q:
    thread['comments']=None
    result['errors']=[{'message':'fixture comments unavailable','path':['repository','pullRequest','reviewThreads','nodes',0,'comments']}]
if mode('partial'): result['errors']=[{'message':'fixture partial','path':['repository','pullRequest','reviewThreads']}]
emit(result)
"#;
