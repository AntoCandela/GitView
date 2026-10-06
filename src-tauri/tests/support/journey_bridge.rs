//! Test-only JSON-line transport to the real service; it is not Tauri IPC or native-picker proof.
//! Requests can select only owned fixtures. Diagnostic capture requires the private host scope
//! and is explicitly unavailable here rather than reported as a fabricated healthy trace.

mod journey_fixture;

use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::time::Duration;

use gitview_lib::application::RepositoryService;
use gitview_lib::browsing::RepositoryFilesRequest;
use gitview_lib::diff::ReviewCategory;
use journey_fixture::JourneyFixture;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    command: String,
    args: Value,
}

#[derive(Clone, Copy)]
enum Choice { Main, Other, Cancel }

struct Bridge {
    fixture: JourneyFixture,
    service: RepositoryService,
    choices: VecDeque<Choice>,
}

fn main() {
    if run().is_err() {
        // Never emit filesystem paths, Git output or arbitrary exception text.
        eprintln!("Journey bridge failed.");
        std::process::exit(1);
    }
}

fn run() -> Result<(), &'static str> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let organization = match arguments.as_slice() {
        [] => false,
        [flag, mode] if flag == "--fixture" && mode == "organization" => true,
        _ => return Err("Invalid journey fixture mode."),
    };
    let fixture = JourneyFixture::create(organization)?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
        .map_err(|_| "Journey runtime failed.")?;
    let requests = read_requests();
    runtime.block_on(Box::pin(async move {
        let service = RepositoryService::with_workspace_file(fixture.workspace_file()).await;
        let mut bridge = Bridge { fixture, service, choices: VecDeque::new() };
        let result = bridge.serve(requests).await;
        bridge.service.shutdown().await;
        result
    }))
}

// A dedicated OS reader must not block the current-thread runtime's observation/recovery tasks.
// It is deliberately not a Tokio blocking task: runtime shutdown must not wait for an open stdin.
fn read_requests() -> tokio::sync::mpsc::Receiver<Result<Vec<u8>, &'static str>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        loop {
            let mut line = Vec::new();
            let read = input.by_ref().take((MAX_REQUEST_BYTES + 1) as u64).read_until(b'\n', &mut line);
            let result = match read {
                Ok(0) => break,
                Ok(_) if line.len() <= MAX_REQUEST_BYTES => Ok(line),
                Ok(_) => Err("Journey request exceeds limit."),
                Err(_) => Err("Journey input failed."),
            };
            let failed = result.is_err();
            if sender.blocking_send(result).is_err() || failed { break; }
        }
    });
    receiver
}

impl Bridge {
    async fn serve(&mut self, mut requests: tokio::sync::mpsc::Receiver<Result<Vec<u8>, &'static str>>) -> Result<(), &'static str> {
        while let Some(line) = requests.recv().await {
            let line = match line {
                Ok(line) => line,
                Err(error) => { respond(0, Err(error))?; break; }
            };
            let request: Request = match serde_json::from_slice(&line) {
                Ok(request) => request,
                Err(_) => { respond(0, Err("Invalid journey request."))?; continue; }
            };
            if !request.args.is_object() {
                respond(request.id, Err("Invalid journey arguments."))?;
                continue;
            }
            let shutdown = request.command == "fixture_shutdown";
            let result = tokio::time::timeout(COMMAND_TIMEOUT, Box::pin(self.dispatch(&request))).await
                .unwrap_or(Err("Journey command timed out."));
            respond(request.id, result)?;
            if shutdown { break; }
        }
        Ok(())
    }

    async fn dispatch(&mut self, request: &Request) -> Result<Value, &'static str> {
        let args = &request.args;
        // Box each service future before embedding it in this dispatch future. Native admission
        // carries substantial state and must not be multiplied through transport stack frames.
        macro_rules! service_result {
            ($future:expr) => {
                serde_json::to_value(Box::pin($future).await).map_err(|_| "Journey serialization failed.")
            };
        }
        match request.command.as_str() {
            "workspace_snapshot" => service_result!(self.service.snapshot()),
            // Explicit isolated fixture, not evidence of the host OS language preference.
            "preferred_languages" => Ok(json!({ "languages": ["en-US"] })),
            "open_chosen_repository" => match self.choices.pop_front().ok_or("No fixture picker choice queued.")? {
                Choice::Main => service_result!(self.service.open_chosen(&self.fixture.main)),
                Choice::Other => service_result!(self.service.open_chosen(&self.fixture.other)),
                Choice::Cancel => service_result!(self.service.cancelled()),
            },
            "select_context" => service_result!(self.service.select(text(args, "entryId")?)),
            "refresh_entry_availability" => service_result!(self.service.refresh(text(args, "entryId")?)),
            "observe_selected_context" => service_result!(self.service.observe_selected_context(text(args, "entryId")?)),
            "review_file" => {
                let revision = args.get("observationRevision").and_then(Value::as_u64).ok_or("Invalid journey arguments.")?;
                let category: ReviewCategory = serde_json::from_value(args.get("category").cloned().ok_or("Invalid journey arguments.")?)
                    .map_err(|_| "Invalid journey arguments.")?;
                service_result!(self.service.review_file(text(args, "entryId")?, revision, text(args, "pathId")?, category))
            }
            "history_page" => service_result!(self.service.history_page(text(args, "entryId")?, optional_text(args, "cursor")?, optional_text(args, "branch")?)),
            "list_contexts" => service_result!(self.service.list_contexts(text(args, "entryId")?)),
            "select_worktree" => service_result!(self.service.select_worktree(text(args, "entryId")?, text(args, "worktreeId")?)),
            "upstream_files" => service_result!(self.service.upstream_files(text(args, "entryId")?, text(args, "token")?)),
            "commit_files" => service_result!(self.service.commit_files(text(args, "entryId")?, text(args, "commitOid")?, optional_text(args, "parentOid")?)),
            "review_commit_file" => service_result!(self.service.review_commit_file(text(args, "entryId")?, text(args, "commitOid")?, optional_text(args, "parentOid")?, text(args, "fileId")?)),
            "list_repository_files" => {
                let request: RepositoryFilesRequest = serde_json::from_value(args.get("request").cloned().ok_or("Invalid journey arguments.")?)
                    .map_err(|_| "Invalid journey arguments.")?;
                service_result!(self.service.list_repository_files(text(args, "entryId")?, request))
            }
            "review_repository_file" => service_result!(self.service.review_repository_file(text(args, "entryId")?, text(args, "listingId")?, text(args, "fileId")?)),
            "rename_repository" => service_result!(self.service.rename(text(args, "entryId")?, text(args, "displayName")?)),
            "remove_repository" => service_result!(self.service.remove(text(args, "entryId")?)),
            "record_renderer_diagnostic" | "diagnostic_health" => Err("Native host diagnostic scope unavailable in journey transport."),
            "fixture_upstream" => self.fixture.upstream(),
            "fixture_info" => Ok(self.fixture.info()),
            "fixture_choose" => {
                if self.choices.len() >= 16 { return Err("Fixture picker queue exceeds limit."); }
                let choice = match text(args, "repository")? {
                    "main" => Choice::Main,
                    "other" => Choice::Other,
                    "cancel" => Choice::Cancel,
                    _ => return Err("Invalid fixture picker choice."),
                };
                self.choices.push_back(choice);
                Ok(Value::Null)
            }
            "fixture_edit" => { self.fixture.edit(text(args, "text")?)?; Ok(Value::Null) }
            "fixture_hide" => { self.fixture.hide()?; Ok(Value::Null) }
            "fixture_restore" => { self.fixture.restore()?; Ok(Value::Null) }
            "fixture_restart" => {
                self.service.shutdown().await;
                self.service = RepositoryService::with_workspace_file(self.fixture.workspace_file()).await;
                self.choices.clear();
                loop {
                    let snapshot = self.service.snapshot().await;
                    if !snapshot.restoring {
                        return serde_json::to_value(snapshot).map_err(|_| "Journey serialization failed.");
                    }
                    tokio::task::yield_now().await;
                }
            }
            "fixture_verify" => self.fixture.verify(),
            "fixture_shutdown" => { self.service.shutdown().await; Ok(Value::Null) }
            _ => Err("Unsupported journey command."),
        }
    }
}

fn text<'a>(args: &'a Value, name: &str) -> Result<&'a str, &'static str> {
    args.get(name).and_then(Value::as_str).ok_or("Invalid journey arguments.")
}

fn optional_text<'a>(args: &'a Value, name: &str) -> Result<Option<&'a str>, &'static str> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err("Invalid journey arguments."),
    }
}

fn respond(id: u64, result: Result<Value, &'static str>) -> Result<(), &'static str> {
    let response = match result {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(error) => json!({ "id": id, "error": error }),
    };
    let bytes = serde_json::to_vec(&response).map_err(|_| "Journey serialization failed.")?;
    let mut output = io::stdout().lock();
    if bytes.len() > MAX_RESPONSE_BYTES {
        let bounded = json!({ "id": id, "error": "Journey response exceeds limit." });
        serde_json::to_writer(&mut output, &bounded).map_err(|_| "Journey output failed.")?;
    } else {
        output.write_all(&bytes).map_err(|_| "Journey output failed.")?;
    }
    output.write_all(b"\n").and_then(|_| output.flush()).map_err(|_| "Journey output failed.")
}
