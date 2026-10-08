//! Exercises coordinator leases against the concrete gh process adapter and independent local Git.
use super::*;
use crate::test_support::{executable, ManualClock};

fn fixture(block: bool) -> (tempfile::TempDir, PrDemandCoordinator, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("gh.pid");
    let program = executable(
        directory.path(),
        r#"
case "$1" in
 version) printf '%s' 'gh version 2.81.0' ;;
 auth) printf '%s' '{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"success"}]}}' ;;
 api)
   if [ "$2" = graphql ]; then
     cat >/dev/null
     printf 'HTTP/2.0 200 OK\nContent-Type: application/json\r\n\r\n%s' '{"data":{"viewer":{"id":"fixture-account"}}}'
   elif [ "$GITVIEW_TEST_BLOCK" = 1 ]; then
     echo $$ > "$GITVIEW_TEST_PID"
     : > "$GITVIEW_TEST_PID.ready"
     exec sleep 60
   else
     printf 'HTTP/2.0 200 OK\nContent-Type: application/json\r\n\r\n%s' '{"number":1,"title":"fixture-private-title"}'
   fi ;;
 *) exit 9 ;;
esac
"#,
    );
    let adapter = GhReadAdapter::fixture(
        &program,
        vec![
            (
                "GH_CONFIG_DIR".into(),
                directory.path().join("config").into_os_string(),
            ),
            ("GH_TOKEN".into(), "fixture-token".into()),
            ("GITHUB_TOKEN".into(), "fixture-token".into()),
            ("GITVIEW_TEST_PID".into(), marker.as_os_str().into()),
            (
                "GITVIEW_TEST_BLOCK".into(),
                if block { "1" } else { "0" }.into(),
            ),
        ],
    );
    (directory, PrDemandCoordinator::new(adapter), marker)
}
fn scope(directory: &std::path::Path) -> DemandScope {
    DemandScope {
        common_storage: directory.to_owned(),
        config_fingerprint: "fixture-config".into(),
        mapped_branch: Some("main".into()),
        identity: Some(crate::github::PrIdentity {
            host: "github.com".into(),
            base_repository_id: "fixture-repository".into(),
            number: 1,
        }),
        version: SnapshotVersion {
            revision: 1,
            base_oid: Some("a".repeat(40)),
            head_oid: Some("b".repeat(40)),
            source: None,
            base_kind: None,
        },
    }
}
fn read() -> GhRead {
    GhRead::ReadPull {
        owner: "fixture".into(),
        repository: "repository".into(),
        number: 1,
    }
}

#[tokio::test]
async fn github_coordinator_concrete_adapter_publishes_only_after_account_observation() {
    let (directory, coordinator, _) = fixture(false);
    let account = coordinator.observe_account().await.unwrap();
    assert_eq!(account.provider_user_id, "fixture-account");
    let mut lease = coordinator
        .request(scope(directory.path()), read())
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while lease.snapshot().loading {
            lease.changed().await;
        }
    })
    .await
    .unwrap();
    let result = lease.snapshot();
    assert!(!result.stale);
    assert_eq!(result.page.unwrap().body["title"], "fixture-private-title");
}

#[tokio::test]
async fn github_coordinator_releases_real_child_while_local_git_remains_independent() {
    let clock = ManualClock::new();
    let (directory, coordinator, marker) = fixture(true);
    clock.finish(coordinator.observe_account()).await.unwrap();
    let work = crate::native_work::NativeWork::default();
    let first = work
        .scope(async {
            coordinator
                .request(scope(directory.path()), read())
                .unwrap()
        })
        .await;
    let survivor = coordinator
        .request(scope(directory.path()), read())
        .unwrap();
    clock
        .wait_for_file(&marker.with_extension("pid.ready"))
        .await;
    let pid = std::fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    drop(first);
    assert!(survivor.snapshot().loading);
    let (_repository, root) = crate::test_support::unborn_working_tree();
    assert!(clock
        .finish(crate::git::GitProbe::default().probe(&root))
        .await
        .is_ok());
    drop(survivor);
    work.close();
    clock.finish(work.drain()).await;
    // Signal zero inspects the fixture child without delivering a signal; no process remains to reap.
    assert!(unsafe {
        libc::kill(pid, 0) == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    });
}
