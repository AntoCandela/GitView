//! Owns bounded gh pipes and child cleanup, including cancellation during wait or termination.
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::OwnedSemaphorePermit;
use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Code, Component, DiagnosticDetails, Event, Level, OperationContext};
use crate::native_work::{self, WorkPermit};
use super::{Failure, GhReadAdapter, PrCode, STDOUT_LIMIT, STDERR_LIMIT};
use super::request::CommandInput;

pub(super) struct Output { pub stdout: Vec<u8>, pub success: bool }
enum ProcessFailure { Start(std::io::ErrorKind), Io, Limit, Deadline }
struct ProcessError { failure: ProcessFailure, cleanup_failed: bool }
impl From<ProcessFailure> for ProcessError { fn from(failure: ProcessFailure) -> Self { Self { failure, cleanup_failed: false } } }

pub(super) async fn run(adapter: &GhReadAdapter, input: CommandInput) -> Result<Output, Failure> {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Process));
    match execute(adapter, input).await {
        Ok((status, stdout, stderr)) => {
            if let Some(trace) = &mut trace { trace.finish(Event::Completed, None, DiagnosticDetails { exit_code: status.code(), stdout_bytes: Some(stdout.len() as u64), stderr_bytes: Some(stderr.len() as u64), ..Default::default() }); }
            Ok(Output { stdout, success: status.success() })
        }
        Err(error) => {
            let (code, diagnostic) = match error.failure {
                ProcessFailure::Start(std::io::ErrorKind::NotFound) => (PrCode::GhMissing, Code::ProcessStart),
                ProcessFailure::Start(_) => (PrCode::GhUnsupported, Code::ProcessStart),
                ProcessFailure::Io => (PrCode::Network, Code::ProcessIo),
                ProcessFailure::Limit => (PrCode::ResourceLimit, Code::OutputLimit),
                ProcessFailure::Deadline => (PrCode::Timeout, Code::Deadline),
            };
            if let Some(trace) = &mut trace { trace.finish(Event::Failed, Some(diagnostic), DiagnosticDetails { cleanup_failed: error.cleanup_failed, ..Default::default() }); }
            Err(code.failure())
        }
    }
}

async fn execute(adapter: &GhReadAdapter, input: CommandInput) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), ProcessError> {
    let deadline = tokio::time::Instant::now() + adapter.deadline;
    let permit = tokio::time::timeout_at(deadline, adapter.permits.clone().acquire_owned()).await
        .map_err(|_| ProcessFailure::Deadline)?.map_err(|_| ProcessFailure::Io)?;
    if tokio::time::Instant::now() >= deadline { return Err(ProcessFailure::Deadline.into()); }
    let mut command = Command::new(&adapter.executable);
    command.args(input.args);
    #[cfg(test)]
    for (key, value) in &adapter.environment { command.env(key, value); }
    for key in ["GH_HOST", "GH_REPO", "GH_DEBUG", "DEBUG", "GH_PAGER", "PAGER", "GH_BROWSER", "BROWSER",
        "GH_FORCE_TTY", "CLICOLOR_FORCE", "GH_COLOR_LABELS", "GH_ACCESSIBLE_COLORS", "GH_FORCE_HYPERLINKS",
        "GH_ACCESSIBLE_PROMPTER"] { command.env_remove(key); }
    for (key, value) in [("GH_PROMPT_DISABLED","1"),("GH_NO_UPDATE_NOTIFIER","1"),("GH_NO_EXTENSION_UPDATE_NOTIFIER","1"),
        ("GH_TELEMETRY","0"),("DO_NOT_TRACK","1"),("NO_COLOR","1"),("CLICOLOR","0"),("GIT_TERMINAL_PROMPT","0")] { command.env(key,value); }
    command.kill_on_drop(true).stdin(if input.input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let work = native_work::current_resource();
    let child = command.spawn().map_err(|e| ProcessFailure::Start(e.kind()))?;
    let mut child = OwnedChild { child: Some(child), permit: Some(permit), work, context: OperationContext::current() };
    let (Some(mut stdout), Some(mut stderr)) = (child.get().stdout.take(), child.get().stderr.take()) else { return Err(child.terminate(ProcessFailure::Io).await); };
    let stdin = child.get().stdin.take();
    if input.input.is_some() && stdin.is_none() { return Err(child.terminate(ProcessFailure::Io).await); }
    let outcome = tokio::time::timeout_at(deadline, async {
        tokio::try_join!(
            async { child.get().wait().await.map_err(|_| ProcessFailure::Io) },
            read_limited(&mut stdout, STDOUT_LIMIT),
            read_limited(&mut stderr, STDERR_LIMIT),
            async move {
                if let (Some(mut stdin), Some(bytes)) = (stdin, input.input) {
                    stdin.write_all(&bytes).await.map_err(|_| ProcessFailure::Io)?;
                    stdin.shutdown().await.map_err(|_| ProcessFailure::Io)?;
                    drop(stdin);
                }
                Ok::<_,ProcessFailure>(())
            }
        )
    }).await;
    match outcome {
        Ok(Ok((status, stdout, stderr, ()))) => { drop(child.child.take()); Ok((status,stdout,stderr)) }
        Ok(Err(failure)) => Err(child.terminate(failure).await),
        Err(_) => Err(child.terminate(ProcessFailure::Deadline).await),
    }
}

struct OwnedChild { child: Option<Child>, permit: Option<OwnedSemaphorePermit>, work: Option<WorkPermit>, context: Option<OperationContext> }
impl OwnedChild {
    fn get(&mut self) -> &mut Child { self.child.as_mut().expect("gh remains owned until reaped") }
    async fn terminate(&mut self, failure: ProcessFailure) -> ProcessError {
        let kill_failed = self.get().start_kill().is_err();
        let reap_failed = self.get().wait().await.is_err();
        // If wait fails, Drop retains cleanup ownership and the permit for another reap attempt.
        if !reap_failed { drop(self.child.take()); }
        let cleanup_failed = kill_failed || reap_failed;
        record_cleanup(self.context.as_ref(), cleanup_failed);
        ProcessError { failure, cleanup_failed }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let kill_failed = child.start_kill().is_err();
            let context = self.context.take();
            let work = self.work.take();
            let permit = self.permit.take();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let (_permit, _work) = (permit, work);
                    let failed = child.wait().await.is_err() || kill_failed;
                    record_cleanup(context.as_ref(), failed);
                });
            } else { record_cleanup(context.as_ref(), true); }
        }
    }
}
fn record_cleanup(context: Option<&OperationContext>, failed: bool) {
    if let Some(context) = context { context.record(if failed {Level::Error} else {Level::Info}, Component::Process,
        if failed {Event::CleanupFailed} else {Event::CleanupCompleted}, failed.then_some(Code::Cleanup), DiagnosticDetails { cleanup_failed: failed, ..Default::default() }); }
}
async fn read_limited(reader: &mut (impl AsyncRead + Unpin), limit: usize) -> Result<Vec<u8>, ProcessFailure> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_to_end(&mut bytes).await.map_err(|_| ProcessFailure::Io)?;
    if bytes.len() > limit { return Err(ProcessFailure::Limit); } Ok(bytes)
}
