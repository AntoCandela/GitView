//! Provides isolated Git/SQLite fixtures and deterministic gates for native concurrency/deadline tests.

// Each test crate selects only the fixtures required by its behavior scenarios.
#![allow(dead_code)]

use std::fs;
#[cfg(unix)]
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

use tempfile::TempDir;

/// Runs fixture mutations with per-command config isolation, never global environment changes.
pub(crate) fn git(at: &Path, arguments: &[&str]) {
    let output = git_output(at, arguments);
    assert!(output.status.success(), "fixture Git {arguments:?} failed: {}", String::from_utf8_lossy(&output.stderr));
}

/// Returns nonzero Git results for scenarios that deliberately create conflicts or failures.
pub(crate) fn git_output(at: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(at)
        .args(arguments)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", at.join(".gitview-empty-global-config"))
        .output()
        .expect("installed Git is needed for native tests")
}

/// Terminates a child without SQLite destructors after durable journal spill, never changing parent env.
pub(crate) fn terminate_diagnostic_transaction_if_requested() {
    let Some(path) = std::env::var_os("GITVIEW_DIAGNOSTIC_CRASH_FIXTURE") else { return; };
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    ).unwrap();
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         UPDATE events SET code='save_failed';
         INSERT INTO events(timestamp_ms,session_id,operation_id,level,component,event)
         SELECT timestamp_ms,session_id,'ec115118-5b62-46d5-a2f4-a2395fa121e4','info','git','completed' FROM events LIMIT 1;
         PRAGMA user_version=1;"
    ).unwrap();
    connection.cache_flush().unwrap();
    // exit bypasses Drop/rollback while the real SQLite transaction and connection remain live.
    std::process::exit(86);
}

pub(crate) fn interrupt_diagnostic_transaction(database: &Path, test_name: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env("GITVIEW_DIAGNOSTIC_CRASH_FIXTURE", database)
        .output().unwrap();
    assert_eq!(output.status.code(), Some(86), "SQLite crash fixture failed: {}", String::from_utf8_lossy(&output.stderr));
}

pub(crate) fn unborn_working_tree() -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-b", "main"]);
    (temp, root)
}

pub(crate) fn working_tree() -> (TempDir, PathBuf) {
    let fixture = unborn_working_tree();
    commit(&fixture.1);
    fixture
}

pub(crate) fn commit(root: &Path) {
    git(root, &["add", "--all"]);
    git(root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "initial"]);
}

#[cfg(unix)]
pub(crate) fn executable(at: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = at.join("isolated-git");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
pub(crate) fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

/// Captures HEAD output before blocking, so later mutations cannot change the stale result.
#[cfg(unix)]
pub(crate) fn gated_git(at: &Path) -> PathBuf {
    executable(at, r#"
if [ "$1 $2 $3" = "symbolic-ref --quiet --short" ] && [ -f .gitview-block ]; then
    git "$@" || exit $?
    printf '%s\n' "$$" > .gitview-child-pid
    : > .gitview-entered
    while [ ! -f .gitview-release ]; do sleep 0.01; done
    exit 0
fi
exec git "$@"
"#)
}

#[cfg(unix)]
pub(crate) fn block(root: &Path) {
    fs::write(root.join(".gitview-block"), b"").unwrap();
}

/// Lets newer probes proceed while an already-entered probe remains blocked until `release`.
#[cfg(unix)]
pub(crate) fn allow_new_probes(root: &Path) {
    fs::remove_file(root.join(".gitview-block")).unwrap();
}

#[cfg(unix)]
pub(crate) fn release(root: &Path) {
    fs::write(root.join(".gitview-release"), b"").unwrap();
}

#[cfg(unix)]
pub(crate) async fn wait_for_probe(root: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.join(".gitview-entered").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("isolated probe did not reach its gate");
}

/// Waits for the recorded child to disappear, not merely for its cancelled task to finish.
#[cfg(unix)]
pub(crate) async fn wait_for_reaped_child(root: &Path) {
    let pid = fs::read_to_string(root.join(".gitview-child-pid")).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while Command::new("kill").args(["-0", pid.trim()]).output().unwrap().status.success() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("cancelled Git child was not killed and reaped");
}

/// Paused Tokio time with explicit advancement while real OS subprocesses reach their gates.
#[cfg(unix)]
pub(crate) struct ManualClock {
    keep_alive: tokio::task::JoinHandle<()>,
}

#[cfg(unix)]
impl ManualClock {
    pub(crate) fn new() -> Self {
        tokio::time::pause();
        // Paused time auto-advances when Tokio is idle, but OS subprocess gates use real time.
        // A runnable task prevents that idle jump from consuming the deadline before release.
        let keep_alive = tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        });
        Self { keep_alive }
    }

    pub(crate) async fn advance(&self, duration: Duration) {
        tokio::time::advance(duration).await;
    }

    pub(crate) async fn wait_for_file(&self, path: &Path) {
        self.finish(async {
            while !path.exists() {
                tokio::task::yield_now().await;
            }
        }).await;
    }

    pub(crate) async fn finish<T>(&self, future: impl Future<Output = T>) -> T {
        // A virtual timeout cannot detect a stuck gate while we intentionally prevent auto-advance.
        let started = std::time::Instant::now();
        tokio::pin!(future);
        loop {
            tokio::select! {
                result = &mut future => return result,
                _ = tokio::task::yield_now() => {
                    assert!(started.elapsed() < Duration::from_secs(15), "deadline test stalled in real time");
                }
            }
        }
    }
}

#[cfg(unix)]
impl Drop for ManualClock {
    fn drop(&mut self) {
        self.keep_alive.abort();
    }
}

/// Holds the first version call, then a later operation, to prove a single shared time budget.
#[cfg(unix)]
pub(crate) fn deadline_git(at: &Path, second_operation: &str) -> PathBuf {
    executable(at, &format!(r#"
if [ "$*" = "--version" ] && [ ! -f {first_entered} ]; then
    : > {first_entered}
    while [ ! -f {first_release} ]; do sleep 0.01; done
elif [ "$*" = "{second_operation}" ]; then
    : > {second_entered}
    while :; do sleep 0.01; done
fi
exec git "$@"
"#,
        first_entered = quote(&at.join("first-entered")),
        first_release = quote(&at.join("first-release")),
        second_entered = quote(&at.join("second-entered")),
    ))
}

struct PendingWorkspaceWrite {
    entered: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    completed: tokio::sync::oneshot::Sender<()>,
}

static WORKSPACE_WRITES: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashMap<PathBuf, PendingWorkspaceWrite>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Holds a prepared write immediately before replacement; dropping it also unblocks the writer.
pub(crate) struct WorkspaceWriteGate {
    path: PathBuf,
    entered: Option<tokio::sync::oneshot::Receiver<()>>,
    release: Option<std::sync::mpsc::Sender<()>>,
    completed: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl WorkspaceWriteGate {
    pub(crate) fn new(path: &Path) -> Self {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release, release_receiver) = std::sync::mpsc::channel();
        let (completed, completed_receiver) = tokio::sync::oneshot::channel();
        assert!(WORKSPACE_WRITES.lock().insert(path.to_owned(), PendingWorkspaceWrite {
            entered, release: release_receiver, completed,
        }).is_none());
        Self {
            path: path.to_owned(), entered: Some(entered_receiver),
            release: Some(release), completed: Some(completed_receiver),
        }
    }

    pub(crate) async fn entered(&mut self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.entered.take().unwrap())
            .await.expect("workspace writer did not reach replacement").unwrap();
    }

    pub(crate) fn release(&mut self) {
        self.release.take().unwrap().send(()).unwrap();
    }

    pub(crate) async fn completed(&mut self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.completed.take().unwrap())
            .await.expect("workspace writer did not complete replacement").unwrap();
    }
}

impl Drop for WorkspaceWriteGate {
    fn drop(&mut self) {
        WORKSPACE_WRITES.lock().remove(&self.path);
        self.release.take();
    }
}

pub(crate) fn before_workspace_persist(path: &Path) -> Option<tokio::sync::oneshot::Sender<()>> {
    let pending = WORKSPACE_WRITES.lock().remove(path)?;
    let _ = pending.entered.send(());
    let _ = pending.release.recv();
    Some(pending.completed)
}
