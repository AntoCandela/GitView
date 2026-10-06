//! Exercises gh through disposable fake executables, never installed credentials or repositories.
use super::*;

fn fixture(body: &str) -> (tempfile::TempDir, GhReadAdapter) {
    let directory = tempfile::tempdir().unwrap();
    let executable = crate::test_support::executable(directory.path(), body);
    let adapter = GhReadAdapter { executable: executable.into_os_string(), permits: Arc::new(Semaphore::new(MAX_ACTIVE)), deadline: CHILD_DEADLINE, environment: Vec::new() };
    (directory, adapter)
}

#[tokio::test]
async fn github_exit_zero_auth_error_is_unavailable() {
    let (_directory, adapter) = fixture(r#"printf '%s' '{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"error","error":"private diagnostic"}]}}'"#);
    assert!(matches!(adapter.read(GhRead::ProbeAuth).await, Err(Failure { code: PrCode::AuthUnavailable, .. })));
}

#[tokio::test]
async fn github_missing_unsupported_and_malformed_gh_are_distinct() {
    let (directory, mut adapter) = fixture("printf '%s' 'gh version 2.80.0'");
    assert!(matches!(adapter.read(GhRead::ProbeVersion).await,Err(Failure{code:PrCode::GhUnsupported,..})));
    adapter.executable = directory.path().join("missing-gh").into_os_string();
    assert!(matches!(adapter.read(GhRead::ProbeVersion).await,Err(Failure{code:PrCode::GhMissing,..})));
    let (_directory, adapter) = fixture("printf '%s' '{bad json}'");
    assert!(matches!(adapter.read(GhRead::ProbeAuth).await,Err(Failure{code:PrCode::InvalidOutput,..})));
    let (_directory, adapter) = fixture("printf '%s' '{\"unsupported_schema\":true}'");
    assert!(matches!(adapter.read(GhRead::ProbeAuth).await,Err(Failure{code:PrCode::GhUnsupported,..})));
}

#[tokio::test]
async fn github_account_probe_uses_json_stdin_and_stable_viewer_identity() {
    let (_directory, adapter) = fixture(r#"
case "$1" in
 version) printf '%s' 'gh version 2.81.0' ;;
 auth) printf '%s' '{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"success","login":"mutable-name"}]}}' ;;
 api) body=$(cat); case "$body" in *'query GitViewViewer { viewer { id } }'*) ;; *) exit 4 ;; esac
      printf 'HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n%s' '{"data":{"viewer":{"id":"stable-user-id"}}}' ;;
 *) exit 9 ;;
esac
"#);
    assert_eq!(adapter.observe_account().await.unwrap_or_else(|_| panic!("valid account must be observed")).provider_user_id,"stable-user-id");
}

#[tokio::test]
async fn github_http_failure_classification_ignores_human_stderr() {
    for (status,headers,expected) in [(403,"X-RateLimit-Remaining: 0\\r\\nRetry-After: 1\\r\\n",PrCode::RateLimited),
        (403,"",PrCode::AccessDenied),(404,"",PrCode::RepositoryUnavailable),(501,"",PrCode::GhUnsupported)] {
        let script = format!("printf 'HTTP/2.0 {status} Status\\r\\n{headers}\\r\\n{{}}'; printf '%s' 'misleading auth token private text' >&2; exit 1");
        let (_directory,adapter) = fixture(&script);
        assert!(matches!(adapter.read(GhRead::ReadViewer).await,Err(Failure{code,..}) if code == expected));
    }
}

#[tokio::test]
async fn github_stdout_and_stderr_overflow_are_independently_bounded() {
    for script in [format!("dd if=/dev/zero bs={} count=1 2>/dev/null",STDOUT_LIMIT+1),
        format!("dd if=/dev/zero bs={} count=1 1>&2 2>/dev/null",STDERR_LIMIT+1)] {
        let (_directory,adapter) = fixture(&script);
        assert!(matches!(adapter.read(GhRead::ProbeAuth).await,Err(Failure{code:PrCode::ResourceLimit,..})));
        assert_eq!(adapter.permits.available_permits(),MAX_ACTIVE);
    }
}

#[tokio::test]
async fn github_exact_stream_limits_are_accepted_without_truncation() {
    let (_directory,adapter) = fixture(&format!("dd if=/dev/zero bs={STDOUT_LIMIT} count=1 2>/dev/null"));
    assert!(matches!(adapter.read(GhRead::ProbeAuth).await,Err(Failure{code:PrCode::InvalidOutput,..})));
    let (_directory,adapter) = fixture(&format!(r#"dd if=/dev/zero bs={STDERR_LIMIT} count=1 1>&2 2>/dev/null
printf '%s' '{{"hosts":{{"github.com":[{{"host":"github.com","active":true,"state":"success"}}]}}}}'"#));
    assert!(matches!(adapter.read(GhRead::ProbeAuth).await,Ok(GhReply::Authenticated)));
}

#[tokio::test]
async fn github_retry_resolves_path_again_without_caching_missing_binary() {
    use std::os::unix::fs::PermissionsExt;
    let (directory,mut adapter) = fixture("exit 99");
    adapter.executable = "gh".into();
    adapter.environment.push(("PATH".into(),directory.path().as_os_str().into()));
    assert!(matches!(adapter.read(GhRead::ProbeVersion).await,Err(Failure{code:PrCode::GhMissing,..})));
    let executable = directory.path().join("gh");
    std::fs::write(&executable,"#!/bin/sh\nprintf '%s' 'gh version 2.81.0'\n").unwrap();
    std::fs::set_permissions(executable,std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(adapter.read(GhRead::ProbeVersion).await,Ok(GhReply::Version)));
}

async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.expect("fixture condition must become observable");
}
fn dead(pid: i32) -> bool {
    // Signal zero checks existence without delivering a signal; ESRCH also proves no zombie remains.
    unsafe { libc::kill(pid,0) == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) }
}
fn pid_file(adapter: &mut GhReadAdapter, directory: &tempfile::TempDir) -> std::path::PathBuf {
    let path = directory.path().join("child.pid");
    adapter.environment.push(("GITVIEW_TEST_PID".into(),path.as_os_str().into())); path
}

#[tokio::test]
async fn github_deadline_kills_and_reaps_the_stalled_child() {
    let clock = crate::test_support::ManualClock::new();
    let (directory,mut adapter) = fixture("echo $$ > \"$GITVIEW_TEST_PID\"; : > \"$GITVIEW_TEST_PID.ready\"; exec sleep 60");
    let path = pid_file(&mut adapter,&directory);
    let permits = adapter.permits.clone();
    let read = tokio::spawn(async move { adapter.read(GhRead::ProbeAuth).await });
    clock.wait_for_file(&path.with_extension("pid.ready")).await;
    clock.advance(Duration::from_secs(31)).await;
    assert!(matches!(clock.finish(read).await.unwrap(),Err(Failure{code:PrCode::Timeout,..})));
    let pid = std::fs::read_to_string(path).unwrap().trim().parse().unwrap();
    assert!(dead(pid));
    assert_eq!(permits.available_permits(),MAX_ACTIVE);
}

#[tokio::test]
async fn github_native_cancellation_retains_cleanup_until_work_drain() {
    use crate::diagnostics::{DiagnosticStore,OperationContext,OperationKind,ReadOnlyDiagnostics,Query,Component,Event};
    let (directory,mut adapter) = fixture("echo $$ > \"$GITVIEW_TEST_PID\"; exec sleep 60");
    let path = pid_file(&mut adapter,&directory);
    let database = directory.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = Arc::new(DiagnosticStore::open(&database));
    let context = OperationContext::new(store.sink(),None,None).with_kind(OperationKind::PrStatus);
    let task_context = context.clone();
    let work = crate::native_work::NativeWork::default();
    let task_work = work.clone();
    let task_adapter = adapter.clone();
    let task = tokio::spawn(async move { task_context.scope(task_work.run(task_adapter.read(GhRead::ProbeAuth))).await });
    until(|| std::fs::read_to_string(&path).ok().and_then(|value| value.trim().parse::<i32>().ok()).is_some()).await;
    let pid = std::fs::read_to_string(path).unwrap().trim().parse().unwrap();
    work.close();
    assert!(task.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(5),work.drain()).await.unwrap();
    assert!(dead(pid));
    assert_eq!(adapter.permits.available_permits(),MAX_ACTIVE);
    let shutting = store.clone(); tokio::task::spawn_blocking(move || shutting.shutdown(Duration::from_secs(5))).await.unwrap().unwrap();
    let events = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query{operation_id:Some(context.id()),limit:200,..Default::default()}).unwrap().events;
    assert!(events.iter().any(|event| event.component == Component::Process && event.event == Event::Cancelled));
    assert!(events.iter().any(|event| event.component == Component::Process && event.event == Event::CleanupCompleted && !event.cleanup_failed));
}

#[tokio::test]
async fn github_unavailable_diagnostics_do_not_replace_domain_failure() {
    use crate::diagnostics::{DiagnosticStore,OperationContext,Code};
    let (_directory,adapter) = fixture(r#"printf '%s' '{"hosts":{}}'"#);
    let store = DiagnosticStore::unavailable(Code::StorageUnavailable);
    let context = OperationContext::new(store.sink(),None,None);
    assert!(matches!(context.scope(adapter.read(GhRead::ProbeAuth)).await,Err(Failure{code:PrCode::AuthRequired,..})));
}

#[tokio::test]
async fn github_concurrency_blocks_a_third_child_and_cancellation_frees_capacity() {
    let (directory,mut adapter) = fixture("echo $$ > \"$GITVIEW_TEST_DIRECTORY/$$\"; exec sleep 60");
    let markers = directory.path().join("markers"); std::fs::create_dir(&markers).unwrap();
    adapter.environment.push(("GITVIEW_TEST_DIRECTORY".into(),markers.as_os_str().into()));
    let mut tasks = Vec::new();
    for _ in 0..2 { let adapter = adapter.clone(); tasks.push(tokio::spawn(async move { adapter.read(GhRead::ProbeAuth).await })); }
    until(|| std::fs::read_dir(&markers).unwrap().count() == 2 && adapter.permits.available_permits() == 0).await;
    let waiting_adapter = adapter.clone();
    tasks.push(tokio::spawn(async move { waiting_adapter.read(GhRead::ProbeAuth).await }));
    // A held permit forces the third read to wait; cancellation must reap before releasing it.
    let first = tasks.remove(0); first.abort(); let _ = first.await;
    until(|| std::fs::read_dir(&markers).unwrap().count() == 3).await;
    for task in tasks { task.abort(); let _ = task.await; }
    until(|| adapter.permits.available_permits() == MAX_ACTIVE).await;
    for entry in std::fs::read_dir(markers).unwrap() { let pid = entry.unwrap().file_name().to_str().unwrap().parse().unwrap(); assert!(dead(pid)); }
    assert!(Arc::ptr_eq(&GhReadAdapter::default().permits,&GhReadAdapter::default().permits));
}

#[tokio::test]
async fn github_child_preserves_credentials_privately_and_diagnostics_store_only_fixed_facts() {
    use crate::diagnostics::{DiagnosticStore,OperationContext,OperationKind,ReadOnlyDiagnostics,Query,Component,Event};
    let (directory,mut adapter) = fixture(r#"
[ "$GH_TOKEN" = "fixture-private-token" ] || exit 10
[ "$GITHUB_TOKEN" = "fixture-fallback-token" ] || exit 11
[ "$GH_HOST$GH_REPO$GH_DEBUG$DEBUG$GH_PAGER$PAGER$GH_BROWSER$BROWSER$GH_FORCE_TTY$CLICOLOR_FORCE" = "" ] || exit 12
[ "$GH_PROMPT_DISABLED$GH_NO_UPDATE_NOTIFIER$GH_NO_EXTENSION_UPDATE_NOTIFIER" = "111" ] || exit 13
[ ! -t 0 ] && [ ! -t 1 ] && [ ! -t 2 ] || exit 14
[ "$GH_CONFIG_DIR" = "$GITVIEW_TEST_CONFIG" ] || exit 15
printf '%s' 'private stderr fixture-private-token' >&2
printf 'HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n%s' '{"data":{"viewer":{"id":"private-provider-id"}},"privateBody":"private-content"}'
"#);
    for key in ["GH_HOST","GH_REPO","GH_DEBUG","DEBUG","GH_PAGER","PAGER","GH_BROWSER","BROWSER","GH_FORCE_TTY","CLICOLOR_FORCE"] { adapter.environment.push((key.into(),"private-override".into())); }
    adapter.environment.extend([("GH_TOKEN".into(),"fixture-private-token".into()),("GITHUB_TOKEN".into(),"fixture-fallback-token".into()),
        ("GH_CONFIG_DIR".into(),directory.path().join("config").into_os_string()),("GITVIEW_TEST_CONFIG".into(),directory.path().join("config").into_os_string())]);
    let database = directory.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = Arc::new(DiagnosticStore::open(&database));
    let context = OperationContext::new(store.sink(),None,None).with_kind(OperationKind::PrStatus);
    assert!(matches!(context.scope(adapter.read(GhRead::ReadViewer)).await,Ok(GhReply::Api(_))));
    let shutting = store.clone(); tokio::task::spawn_blocking(move || shutting.shutdown(Duration::from_secs(5))).await.unwrap().unwrap();
    let events = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query{operation_id:Some(context.id()),limit:200,..Default::default()}).unwrap().events;
    assert!(events.iter().all(|event| event.component == Component::Process && event.operation_id == context.id()));
    assert!(events.iter().any(|event| event.event == Event::Completed && event.stdout_bytes.is_some_and(|n| n > 0) && event.stderr_bytes.is_some_and(|n| n > 0)));
    let bytes = std::fs::read(database).unwrap();
    for private in ["fixture-private-token","fixture-fallback-token","private-provider-id","private-content","private-override",directory.path().to_str().unwrap()] {
        assert!(!bytes.windows(private.len()).any(|window| window == private.as_bytes()));
    }
}
