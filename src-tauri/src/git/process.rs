//! Executes internal Git operations with bounded output, a shared deadline and owned children.
//!
//! The probe chooses fixed arguments and interprets results; this adapter isolates
//! process mechanics and preserves Git's ownership protection.

use std::ffi::OsString;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::time::Instant;


use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Code, Component, DiagnosticDetails, Event, Level, OperationContext};
use crate::native_work::{self, WorkPermit};
const OUTPUT_LIMIT: usize = 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Absolute operation deadline; copying it shares the remaining budget instead of resetting it.
#[derive(Clone, Copy)]
pub(crate) struct ProbeDeadline(Instant);

impl ProbeDeadline {
    pub(crate) fn new() -> Self {
        Self(Instant::now() + PROBE_TIMEOUT)
    }

    pub(crate) fn instant(self) -> Instant { self.0 }

    pub(crate) fn check(self) -> Result<(), ProcessError> {
        if Instant::now() >= self.0 {
            Err(ProcessError::new(ProcessFailure::Deadline))
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn after(duration: Duration) -> Self {
        Self(Instant::now() + duration)
    }
}

#[derive(Debug)]
pub(crate) enum ProcessFailure {
    Start(std::io::Error),
    Io(std::io::Error),
    OutputLimit,
    Deadline,
}

/// Primary process failure plus explicit termination failures, for honest error classification.
#[derive(Debug)]
pub(crate) struct ProcessError {
    pub(crate) failure: ProcessFailure,
    kill_error: Option<std::io::Error>,
    reap_error: Option<std::io::Error>,
}

impl ProcessError {
    fn new(failure: ProcessFailure) -> Self {
        Self {
            failure,
            kill_error: None,
            reap_error: None,
        }
    }

    pub(crate) fn cleanup_failed(&self) -> bool {
        self.kill_error.is_some() || self.reap_error.is_some()
    }
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}", self)
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.failure {
            ProcessFailure::Start(error) | ProcessFailure::Io(error) => Some(error),
            _ => self.kill_error.as_ref().or(self.reap_error.as_ref()).map(|error| {
                error as &(dyn std::error::Error + 'static)
            }),
        }
    }
}

/// Raw, independently bounded streams; decoding and exit-code meaning belong to the probe.
pub(crate) struct ProcessOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Native process adapter, never exposed as a renderer-supplied command interface.
#[derive(Clone)]
pub(crate) struct GitProcess {
    executable: OsString,
}

impl Default for GitProcess {
    fn default() -> Self {
        Self {
            executable: OsString::from("git"),
        }
    }
}

impl GitProcess {
    /// Runs probe-owned arguments; deadline/output/I/O failure requests termination and waits.
    ///
    /// Dropping the future delegates reap to the current runtime rather than abandoning
    /// a running child. Output limits apply independently to stdout and stderr.
    pub(crate) async fn run(
        &self, at: Option<&Path>, arguments: &[&str], deadline: ProbeDeadline,
    ) -> Result<ProcessOutput, ProcessError> {
        self.run_with_input(at, arguments, deadline, None).await
    }


    /// Larger stdout is allowed only for host-owned copied-index enumeration; stderr stays bounded.
    pub(crate) async fn run_isolated_with_output_limit(
        &self, at: Option<&Path>, arguments: &[&str], deadline: ProbeDeadline, stdout_limit: usize,
    ) -> Result<ProcessOutput, ProcessError> {
        self.run_with_policy(at, arguments, deadline, None, true, stdout_limit).await
    }

    /// Batch stdin is host-owned validated data, bounded like either captured stream.
    pub(crate) async fn run_with_input(
        &self, at: Option<&Path>, arguments: &[&str], deadline: ProbeDeadline, input: Option<&[u8]>,
    ) -> Result<ProcessOutput, ProcessError> {
        self.run_with_policy(at, arguments, deadline, input, false, OUTPUT_LIMIT).await
    }

    async fn run_with_policy(
        &self, at: Option<&Path>, arguments: &[&str], deadline: ProbeDeadline,
        input: Option<&[u8]>, isolated: bool, stdout_limit: usize,
    ) -> Result<ProcessOutput, ProcessError> {
        let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Process));
        let result = if input.is_some_and(|bytes| bytes.len() > OUTPUT_LIMIT) {
            Err(ProcessError::new(ProcessFailure::OutputLimit))
        } else {
            self.run_inner(at, arguments, deadline, input, isolated, stdout_limit).await
        };
        if let Some(trace) = &mut trace {
            match &result {
                Ok(output) => trace.finish(Event::Completed, None, DiagnosticDetails {
                    exit_code: output.status.code(),
                    stdout_bytes: Some(output.stdout.len() as u64),
                    stderr_bytes: Some(output.stderr.len() as u64),
                    ..Default::default()
                }),
                Err(error) => trace.finish(Event::Failed, Some(match &error.failure {
                    ProcessFailure::Start(_) => Code::ProcessStart,
                    ProcessFailure::Io(_) => Code::ProcessIo,
                    ProcessFailure::OutputLimit => Code::OutputLimit,
                    ProcessFailure::Deadline => Code::Deadline,
                }), DiagnosticDetails { cleanup_failed: error.cleanup_failed(), ..Default::default() }),
            }
        }
        result
    }

    async fn run_inner(
        &self,
        at: Option<&Path>,
        arguments: &[&str],
        deadline: ProbeDeadline,
        input: Option<&[u8]>,
        isolated: bool,
        stdout_limit: usize,
    ) -> Result<ProcessOutput, ProcessError> {
        deadline.check()?;
        let mut command = Command::new(&self.executable);
        command.args(arguments);
        if let Some(at) = at {
            command.current_dir(at);
        }
        // Inherited Git overrides must not redirect the selected directory to another repository.
        for variable in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_NAMESPACE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
            "GIT_EXTERNAL_DIFF",
            "GIT_DIFF_OPTS",
            "GIT_ATTR_SOURCE",
            "GIT_GLOB_PATHSPECS",
            "GIT_NOGLOB_PATHSPECS",
            "GIT_ICASE_PATHSPECS",
        ] {
            command.env_remove(variable);
        }
        // Inspection must neither take optional write locks nor wait for interactive credentials.
        command.env("GIT_OPTIONAL_LOCKS", "0");
        command.env("GIT_TERMINAL_PROMPT", "0");
        command.env("GIT_LITERAL_PATHSPECS", "1");
        command.env("GIT_NO_LAZY_FETCH", "1");
        if isolated {
            // The private metadata contains the complete comparison configuration.
            let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
            command.env("GIT_CONFIG_NOSYSTEM", "1");
            command.env("GIT_CONFIG_SYSTEM", null);
            command.env("GIT_CONFIG_GLOBAL", null);
            command.env_remove("GIT_CONFIG");
        }
        // Error classification matches Git's English ownership/permission diagnostics.
        command.env("LC_ALL", "C");
        command.kill_on_drop(true);
        command
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let work = native_work::current_resource();
        let child = command
            .spawn()
            .map_err(|error| ProcessError::new(ProcessFailure::Start(error)))?;
        let mut child = ReapingChild { child: Some(child), context: OperationContext::current(), work };
        let (Some(mut stdout), Some(mut stderr)) = (
            child.child_mut().stdout.take(),
            child.child_mut().stderr.take(),
        ) else {
            return Err(child
                .terminate(ProcessFailure::Io(std::io::Error::other("Missing Git output pipe")))
                .await);
        };
        let stdin = child.child_mut().stdin.take();
        if input.is_some() && stdin.is_none() {
            return Err(child.terminate(ProcessFailure::Io(std::io::Error::other("Missing Git input pipe"))).await);
        }
        let outcome = tokio::time::timeout_at(deadline.0, async {
            // Drain both pipes while waiting: sequential reads can deadlock on a full other pipe.
            tokio::try_join!(
                async { child.child_mut().wait().await.map_err(ProcessFailure::Io) },
                read_limited(&mut stdout, stdout_limit),
                read_limited(&mut stderr, OUTPUT_LIMIT),
                async move {
                    if let (Some(mut stdin), Some(input)) = (stdin, input) {
                        stdin.write_all(input).await.map_err(ProcessFailure::Io)?;
                        stdin.shutdown().await.map_err(ProcessFailure::Io)?;
                        // The owned pipe must close here: batch Git waits for EOF before exiting.
                        drop(stdin);
                    }
                    Ok::<_, ProcessFailure>(())
                }
            )
        })
        .await;
        match outcome {
            Ok(Ok((status, stdout, stderr, ()))) => {
                // wait completed; disarm the cancellation guard only after reaping.
                drop(child.child.take());
                Ok(ProcessOutput {
                    status,
                    stdout,
                    stderr,
                })
            }
            Ok(Err(failure)) => Err(child.terminate(failure).await),
            Err(_) => Err(child.terminate(ProcessFailure::Deadline).await),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_executable(executable: &Path) -> Self {
        Self {
            executable: executable.as_os_str().to_owned(),
        }
    }
}

/// Owns the child until waited, including if the probe future is dropped during cleanup.
struct ReapingChild {
    child: Option<Child>,
    context: Option<OperationContext>,
    work: Option<WorkPermit>,
}

impl ReapingChild {
    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("Git child is owned until reaped")
    }

    async fn terminate(&mut self, failure: ProcessFailure) -> ProcessError {
        // Requesting termination is not reaping; retain ownership through wait and report both errors.
        let kill_error = self.child_mut().start_kill().err();
        let reap_error = self.child_mut().wait().await.err();
        drop(self.child.take());
        self.record_cleanup(kill_error.is_some() || reap_error.is_some());
        ProcessError {
            failure,
            kill_error,
            reap_error,
        }
    }

    fn record_cleanup(&self, failed: bool) {
        if let Some(context) = &self.context {
            context.record(if failed { Level::Error } else { Level::Info }, Component::Process,
                if failed { Event::CleanupFailed } else { Event::CleanupCompleted },
                failed.then_some(Code::Cleanup), DiagnosticDetails { cleanup_failed: failed, ..Default::default() });
        }
    }
}

impl Drop for ReapingChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Drop requests termination now; only a completed wait can report cleanup complete.
            let kill_failed = child.start_kill().is_err();
            let context = self.context.take();
            let work = self.work.take();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _work = work;
                    let failed = child.wait().await.is_err() || kill_failed;
                    if let Some(context) = context {
                        context.record(if failed { Level::Error } else { Level::Info }, Component::Process,
                            if failed { Event::CleanupFailed } else { Event::CleanupCompleted },
                            failed.then_some(Code::Cleanup), DiagnosticDetails { cleanup_failed: failed, ..Default::default() });
                    }
                });
            } else if let Some(context) = context {
                context.record(Level::Error, Component::Process, Event::CleanupFailed, Some(Code::Cleanup),
                    DiagnosticDetails { cleanup_failed: true, ..Default::default() });
            }
        }
    }
}

async fn read_limited(reader: &mut (impl AsyncRead + Unpin), limit: usize) -> Result<Vec<u8>, ProcessFailure> {
    let mut collected = Vec::new();
    // Read one extra byte to distinguish an exact-limit response from truncated overflow.
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut collected)
        .await
        .map_err(ProcessFailure::Io)?;
    if collected.len() > limit {
        return Err(ProcessFailure::OutputLimit);
    }
    Ok(collected)
}

#[cfg(test)]
#[path = "../../tests/unit/process.rs"]
mod unit_tests;
