//! Reads pinned ref seeds and complete raw commit parent headers with bounded upstream fetches on first-page reads.

use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::Arc};
use super::*;
use crate::git::RepositoryKind;
use crate::git::process::{ProbeDeadline, ProcessError, ProcessFailure, ProcessOutput};
use crate::workspace::NativeIdentity;

const MAX_REFS: usize = 1024;
const MAX_PARENTS_PER_COMMIT: usize = 64;
const MAX_SUBJECT_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RawRef { pub(crate) kind: RefKind, pub(crate) name: String, oid: String, object_type: String }
const PREFIX: &[&str] = &["--no-optional-locks", "--no-replace-objects", "-c", "core.fsmonitor=false"];

pub(super) async fn read_page(process: &GitProcess, context: &SelectedContext, ticket: &HistoryTicket) -> Result<HistoryCandidate, HistoryErrorCode> {
    verify_context(process, context, ProbeDeadline::new()).await?;
    let tracking = if ticket.snapshot.is_none() { upstream::fetch(process, context, ticket.branch.as_deref()).await } else { Ok(None) };
    let deadline = ProbeDeadline::new();
    let snapshot = match &ticket.snapshot {
        Some(snapshot) => Arc::clone(snapshot),
        None => {
            let mut snapshot = capture(process, context, ticket.branch.as_deref(), deadline).await?;
            snapshot.upstream = upstream::summarize(process, context, &snapshot, ticket.branch.as_deref(), tracking, deadline).await;
            if ticket.branch.is_some() {
                if let Some(range) = &snapshot.upstream.incoming { snapshot.seeds.push(range.tip_oid.clone()); }
            }
            Arc::new(snapshot)
        },
    };
    let (commits, has_more, missing) = traverse(process, context, &snapshot, ticket.offset, deadline).await?;
    verify_context(process, context, deadline).await?;
    Ok(HistoryCandidate { page: HistoryPage {
        entry_id: context.entry_id.clone(), cursor: None, commits, refs: snapshot.refs.clone(), head: snapshot.head.clone(), upstream: snapshot.upstream.clone(), has_more,
        completeness: if snapshot.shallow || missing { Completeness::ShallowOrMissing } else if has_more { Completeness::Paged } else { Completeness::Complete },
    }, snapshot })
}

pub(super) async fn run(process: &GitProcess, root: &Path, arguments: &[&str], input: Option<&[u8]>, deadline: ProbeDeadline) -> Result<ProcessOutput, HistoryErrorCode> {
    let mut fixed = Vec::with_capacity(PREFIX.len() + arguments.len());
    fixed.extend_from_slice(PREFIX);
    fixed.extend_from_slice(arguments);
    process.run_with_input(Some(root), &fixed, deadline, input).await.map_err(process_error)
}
pub(crate) async fn required(process: &GitProcess, root: &Path, arguments: &[&str], input: Option<&[u8]>, deadline: ProbeDeadline) -> Result<Vec<u8>, HistoryErrorCode> {
    let output = run(process, root, arguments, input, deadline).await?;
    if !output.status.success() { return Err(exit_error(&output.stderr)); }
    Ok(output.stdout)
}

pub(crate) async fn verify_context(process: &GitProcess, context: &SelectedContext, deadline: ProbeDeadline) -> Result<(), HistoryErrorCode> {
    verify_identity(context, deadline).await?;
    let git_dir = required(process, &context.root, &["rev-parse", "--absolute-git-dir"], None, deadline).await?;
    let bare = required(process, &context.root, &["rev-parse", "--is-bare-repository"], None, deadline).await?;
    let expected_bare = if context.kind == RepositoryKind::Bare { b"true\n".as_slice() } else { b"false\n".as_slice() };
    if bare != expected_bare { return Err(HistoryErrorCode::Inaccessible); }
    let root = if context.kind == RepositoryKind::Bare { git_dir.clone() }
        else { required(process, &context.root, &["rev-parse", "--show-toplevel"], None, deadline).await? };
    let root = output_path(&root)?;
    let git_dir = output_path(&git_dir)?;
    let task = crate::native_work::spawn_blocking(move || Ok::<_, HistoryErrorCode>((
        std::fs::canonicalize(root).map_err(|_| HistoryErrorCode::Inaccessible)?,
        std::fs::canonicalize(git_dir).map_err(|_| HistoryErrorCode::Inaccessible)?,
    )));
    let (root, git_dir) = tokio::time::timeout_at(deadline.instant(), task).await.map_err(|_| HistoryErrorCode::Timeout)?
        .map_err(|_| HistoryErrorCode::Inaccessible)??;
    if root != context.root || git_dir != context.git_dir { return Err(HistoryErrorCode::Inaccessible); }
    verify_identity(context, deadline).await
}
fn output_path(bytes: &[u8]) -> Result<PathBuf, HistoryErrorCode> {
    let bytes = bytes.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?;
    let text = std::str::from_utf8(bytes).map_err(|_| HistoryErrorCode::InvalidOutput)?;
    let path = PathBuf::from(text);
    if !path.is_absolute() || bytes.contains(&0) { return Err(HistoryErrorCode::InvalidOutput); }
    Ok(path)
}
async fn verify_identity(context: &SelectedContext, deadline: ProbeDeadline) -> Result<(), HistoryErrorCode> {
    let root = context.root.clone();
    let git_dir = context.git_dir.clone();
    let task = crate::native_work::spawn_blocking(move || NativeIdentity::capture(&root, &git_dir));
    let identity = tokio::time::timeout_at(deadline.instant(), task).await.map_err(|_| HistoryErrorCode::Timeout)?
        .map_err(|_| HistoryErrorCode::Inaccessible)?.map_err(|_| HistoryErrorCode::Inaccessible)?;
    if identity != context.identity { return Err(HistoryErrorCode::Inaccessible); }
    Ok(())
}

async fn capture(process: &GitProcess, context: &SelectedContext, viewed_branch: Option<&str>, deadline: ProbeDeadline) -> Result<PinnedSnapshot, HistoryErrorCode> {
    let symbolic = run(process, &context.root, &["symbolic-ref", "--quiet", "HEAD"], None, deadline).await?;
    let branch = if symbolic.status.success() {
        let text = std::str::from_utf8(symbolic.stdout.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?).map_err(|_| HistoryErrorCode::InvalidOutput)?;
        if !valid_ref_name(text) { return Err(HistoryErrorCode::InvalidOutput); }
        text.strip_prefix("refs/heads/").map(str::to_owned)
    } else { None };
    let output = required(process, &context.root, &[
        "for-each-ref", "--count=1025", "--format=%(refname)%00%(objectname)%00%(objecttype)%00",
        "refs/heads/", "refs/remotes/", "refs/tags/",
    ], None, deadline).await?;
    let records = parse_refs(&output)?;
    let mut refs = Vec::with_capacity(records.len());
    for RawRef { kind, name, oid, object_type } in records {
        deadline.check().map_err(process_error)?;
        let commit_oid = match object_type.as_str() {
            "commit" => oid,
            "tag" => {
                // Peeling the captured object ID, not its moving ref name, pins annotated tags.
                let object = format!("{oid}^{{}}");
                let peeled = required(process, &context.root, &["rev-parse", "--verify", &object], None, deadline).await?;
                let peeled_oid = output_oid(&peeled)?;
                let object_type = required(process, &context.root, &["cat-file", "-t", peeled_oid], None, deadline).await?;
                match object_type.as_slice() {
                    b"commit\n" => peeled_oid.to_owned(),
                    b"blob\n" | b"tree\n" => continue,
                    _ => return Err(HistoryErrorCode::InvalidOutput),
                }
            }
            "tree" | "blob" => continue,
            _ => return Err(HistoryErrorCode::InvalidOutput),
        };
        refs.push(HistoryRef { kind, name, commit_oid });
    }
    let scope = if context.kind == RepositoryKind::Bare { HeadScope::Repository } else { HeadScope::Worktree };
    let head = if let Some(branch) = branch {
        let oid = refs.iter().find(|reference| reference.kind == RefKind::LocalBranch && reference.name == branch).map(|reference| reference.commit_oid.clone());
        let state = if oid.is_some() { HeadState::Attached } else {
            let target = format!("refs/heads/{branch}");
            let existence = run(process, &context.root, &["show-ref", "--verify", "--quiet", &target], None, deadline).await?;
            if existence.status.code() == Some(1) { HeadState::Unborn } else { HeadState::Unresolved }
        };
        HistoryHead { scope, state, branch: Some(branch), oid }
    } else {
        let verified = run(process, &context.root, &["rev-parse", "--verify", "HEAD^{commit}"], None, deadline).await?;
        let oid = if verified.status.success() { Some(output_oid(&verified.stdout)?.to_owned()) } else { None };
        HistoryHead { scope, state: if symbolic.status.code() == Some(1) && oid.is_some() { HeadState::Detached } else { HeadState::Unresolved }, branch: None, oid }
    };
    let shallow = required(process, &context.root, &["rev-parse", "--is-shallow-repository"], None, deadline).await?;
    let shallow = match shallow.as_slice() { b"true\n" => true, b"false\n" => false, _ => return Err(HistoryErrorCode::InvalidOutput) };
    let mut seeds = Vec::with_capacity(refs.len() + 1);
    if let Some(branch) = viewed_branch {
        let reference = refs.iter().find(|reference| reference.kind == RefKind::LocalBranch && reference.name == branch)
            .ok_or(HistoryErrorCode::Inaccessible)?;
        seeds.push(reference.commit_oid.clone());
    } else {
        let mut unique = HashSet::new();
        for oid in refs.iter().map(|reference| &reference.commit_oid).chain(head.oid.iter()) {
            if unique.insert(oid.as_str()) { seeds.push(oid.clone()); }
        }
    }
    Ok(PinnedSnapshot { upstream: UpstreamSummary::empty(UpstreamState::NoUpstream, None), refs, head, seeds, shallow })
}

pub(crate) fn parse_refs(bytes: &[u8]) -> Result<Vec<RawRef>, HistoryErrorCode> {
    if bytes.is_empty() { return Ok(Vec::new()); }
    let body = bytes.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?;
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for record in body.split(|byte| *byte == b'\n') {
        if refs.len() == MAX_REFS { return Err(HistoryErrorCode::ResourceLimit); }
        let fields = fields::<4>(record, 0)?;
        if !fields[3].is_empty() { return Err(HistoryErrorCode::InvalidOutput); }
        let name = std::str::from_utf8(fields[0]).map_err(|_| HistoryErrorCode::InvalidOutput)?;
        if !valid_ref_name(name) || !seen.insert(name) { return Err(HistoryErrorCode::InvalidOutput); }
        let (kind, label) = if let Some(name) = name.strip_prefix("refs/heads/") { (RefKind::LocalBranch, name) }
            else if let Some(name) = name.strip_prefix("refs/remotes/") { (RefKind::RemoteTracking, name) }
            else if let Some(name) = name.strip_prefix("refs/tags/") { (RefKind::Tag, name) }
            else { return Err(HistoryErrorCode::InvalidOutput); };
        if label.is_empty() { return Err(HistoryErrorCode::InvalidOutput); }
        let oid = oid(fields[1])?;
        let object_type = std::str::from_utf8(fields[2]).map_err(|_| HistoryErrorCode::InvalidOutput)?;
        refs.push(RawRef { kind, name: label.to_owned(), oid: oid.to_owned(), object_type: object_type.to_owned() });
    }
    Ok(refs)
}

fn fields<const N: usize>(bytes: &[u8], separator: u8) -> Result<[&[u8]; N], HistoryErrorCode> {
    let mut source = bytes.split(|byte| *byte == separator);
    let mut fields = [b"".as_slice(); N];
    for field in &mut fields { *field = source.next().ok_or(HistoryErrorCode::InvalidOutput)?; }
    if source.next().is_some() { return Err(HistoryErrorCode::InvalidOutput); }
    Ok(fields)
}
pub(crate) fn valid_ref_name(name: &str) -> bool {
    !name.is_empty() && !name.bytes().any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\'))
        && !name.contains("..") && !name.contains("@{") && !name.split('/').any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock") || part.ends_with('.'))
}
pub(super) fn output_oid(bytes: &[u8]) -> Result<&str, HistoryErrorCode> { oid(bytes.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?) }
pub(crate) fn oid(bytes: &[u8]) -> Result<&str, HistoryErrorCode> {
    if !matches!(bytes.len(), 40 | 64) || !bytes.iter().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')) { return Err(HistoryErrorCode::InvalidOutput); }
    std::str::from_utf8(bytes).map_err(|_| HistoryErrorCode::InvalidOutput)
}
fn oid_input<'a>(oids: impl IntoIterator<Item = &'a str>) -> Vec<u8> {
    let mut input = Vec::new();
    for oid in oids { input.extend_from_slice(oid.as_bytes()); input.push(b'\n'); }
    input
}

async fn traverse(process: &GitProcess, context: &SelectedContext, snapshot: &PinnedSnapshot, offset: usize, deadline: ProbeDeadline) -> Result<(Vec<HistoryCommit>, bool, bool), HistoryErrorCode> {
    if snapshot.seeds.is_empty() { return Ok((Vec::new(), false, snapshot.shallow)); }
    let input = oid_input(snapshot.seeds.iter().map(String::as_str));
    let skip = format!("--skip={offset}");
    let output = required(process, &context.root, &["rev-list", "--topo-order", "--parents", "--max-count=101", &skip, "--stdin"], Some(&input), deadline).await?;
    let mut rows = parse_traversal(&output)?;
    let has_more = rows.len() > PAGE_SIZE;
    rows.truncate(PAGE_SIZE);
    if rows.is_empty() { return Ok((Vec::new(), false, snapshot.shallow)); }
    let input = oid_input(rows.iter().map(String::as_str));
    let raw = required(process, &context.root, &["cat-file", "--batch"], Some(&input), deadline).await?;
    let mut commits = parse_batch(&raw, &rows)?;
    let loaded: HashSet<_> = rows.iter().map(String::as_str).collect();
    let mut parents = HashSet::new();
    for commit in &commits {
        for parent in &commit.parents { if !loaded.contains(parent.oid.as_str()) { parents.insert(parent.oid.as_str()); } }
    }
    let input = oid_input(parents.iter().copied());
    let available = if input.is_empty() { HashMap::new() } else {
        let output = required(process, &context.root, &["cat-file", "--batch-check"], Some(&input), deadline).await?;
        parse_available(&output, &parents)?
    };
    let mut missing = false;
    for commit in &mut commits {
        for parent in &mut commit.parents {
            parent.state = if loaded.contains(parent.oid.as_str()) { ParentState::Loaded }
                else if available.get(parent.oid.as_str()) == Some(&true) { ParentState::OutsidePage }
                else { missing = true; ParentState::Unavailable };
        }
    }
    deadline.check().map_err(process_error)?;
    Ok((commits, has_more, missing))
}

pub(super) fn parse_traversal(bytes: &[u8]) -> Result<Vec<String>, HistoryErrorCode> {
    if bytes.is_empty() { return Ok(Vec::new()); }
    let body = bytes.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?;
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for row in body.split(|byte| *byte == b'\n') {
        if rows.len() == PAGE_SIZE + 1 { return Err(HistoryErrorCode::ResourceLimit); }
        let mut fields = row.split(|byte| *byte == b' ');
        let commit = oid(fields.next().ok_or(HistoryErrorCode::InvalidOutput)?)?;
        if !seen.insert(commit) { return Err(HistoryErrorCode::InvalidOutput); }
        for parent in fields { oid(parent)?; }
        rows.push(commit.to_owned());
    }
    Ok(rows)
}

pub(crate) fn parse_batch(bytes: &[u8], requested: &[String]) -> Result<Vec<HistoryCommit>, HistoryErrorCode> {
    let mut remaining = bytes;
    let mut commits = Vec::with_capacity(requested.len());
    for expected in requested {
        let newline = remaining.iter().position(|byte| *byte == b'\n').ok_or(HistoryErrorCode::InvalidOutput)?;
        let header = std::str::from_utf8(&remaining[..newline]).map_err(|_| HistoryErrorCode::InvalidOutput)?;
        remaining = &remaining[newline + 1..];
        let mut fields = header.split(' ');
        let actual = fields.next().ok_or(HistoryErrorCode::InvalidOutput)?;
        let object_type = fields.next().ok_or(HistoryErrorCode::InvalidOutput)?;
        let size = fields.next();
        if actual != expected || fields.next().is_some() { return Err(HistoryErrorCode::InvalidOutput); }
        if object_type == "missing" && size.is_none() { return Err(HistoryErrorCode::MissingObjects); }
        let size = size.ok_or(HistoryErrorCode::InvalidOutput)?;
        if object_type != "commit" || size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_digit()) { return Err(HistoryErrorCode::InvalidOutput); }
        let length: usize = size.parse().map_err(|_| HistoryErrorCode::InvalidOutput)?;
        if remaining.len() <= length || remaining[length] != b'\n' { return Err(HistoryErrorCode::InvalidOutput); }
        commits.push(parse_commit(expected, &remaining[..length])?);
        remaining = &remaining[length + 1..];
    }
    if !remaining.is_empty() { return Err(HistoryErrorCode::InvalidOutput); }
    Ok(commits)
}

pub(super) fn parse_commit(commit_oid: &str, bytes: &[u8]) -> Result<HistoryCommit, HistoryErrorCode> {
    oid(commit_oid.as_bytes())?;
    let separator = bytes.windows(2).position(|bytes| bytes == b"\n\n").ok_or(HistoryErrorCode::InvalidOutput)?;
    let mut headers = bytes[..separator].split(|byte| *byte == b'\n');
    let tree = headers.next().and_then(|line| line.strip_prefix(b"tree ")).ok_or(HistoryErrorCode::InvalidOutput)?;
    oid(tree)?;
    if tree.len() != commit_oid.len() { return Err(HistoryErrorCode::InvalidOutput); }
    let mut parents = Vec::new();
    let mut author = false;
    let mut committer = false;
    let mut utf8_encoding = true;
    let mut parents_finished = false;
    for header in headers {
        if let Some(parent) = header.strip_prefix(b"parent ") {
            if parents_finished { return Err(HistoryErrorCode::InvalidOutput); }
            if parents.len() == MAX_PARENTS_PER_COMMIT { return Err(HistoryErrorCode::ResourceLimit); }
            if parent.len() != commit_oid.len() { return Err(HistoryErrorCode::InvalidOutput); }
            parents.push(HistoryParent { oid: oid(parent)?.to_owned(), state: ParentState::OutsidePage });
        } else {
            parents_finished = true;
            if header.starts_with(b"author ") { author = true; }
            else if header.starts_with(b"committer ") { committer = true; }
            else if let Some(encoding) = header.strip_prefix(b"encoding ") {
                utf8_encoding = encoding.eq_ignore_ascii_case(b"utf-8") || encoding.eq_ignore_ascii_case(b"utf8") || encoding.eq_ignore_ascii_case(b"ascii");
            } else if !header.starts_with(b" ") && !header.split(|byte| *byte == b' ').next().is_some_and(|key| !key.is_empty() && key.iter().all(|byte| byte.is_ascii_alphabetic() || *byte == b'-')) {
                return Err(HistoryErrorCode::InvalidOutput);
            }
        }
    }
    if !author || !committer { return Err(HistoryErrorCode::InvalidOutput); }
    let subject = bytes[separator + 2..].split(|byte| *byte == b'\n').next().unwrap_or_default();
    let subject = if utf8_encoding && subject.len() <= MAX_SUBJECT_BYTES && !subject.contains(&0) {
        std::str::from_utf8(subject).ok().map(str::to_owned)
    } else { None };
    Ok(HistoryCommit { oid: commit_oid.to_owned(), subject, root: parents.is_empty(), parents })
}
fn parse_available(bytes: &[u8], requested: &HashSet<&str>) -> Result<HashMap<String, bool>, HistoryErrorCode> {
    let mut result = HashMap::with_capacity(requested.len());
    if bytes.is_empty() { return Err(HistoryErrorCode::InvalidOutput); }
    let body = bytes.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?;
    for row in body.split(|byte| *byte == b'\n') {
        let mut fields = row.split(|byte| *byte == b' ');
        let commit = oid(fields.next().ok_or(HistoryErrorCode::InvalidOutput)?)?;
        if !requested.contains(commit) || result.contains_key(commit) { return Err(HistoryErrorCode::InvalidOutput); }
        let object_type = fields.next().ok_or(HistoryErrorCode::InvalidOutput)?;
        let length = fields.next();
        if fields.next().is_some() { return Err(HistoryErrorCode::InvalidOutput); }
        let present = match (object_type, length) {
            (b"missing", None) => false,
            (b"commit", Some(length)) if !length.is_empty() && length.iter().all(u8::is_ascii_digit) => true,
            (b"blob" | b"tree" | b"tag", Some(length)) if !length.is_empty() && length.iter().all(u8::is_ascii_digit) => false,
            _ => return Err(HistoryErrorCode::InvalidOutput),
        };
        result.insert(commit.to_owned(), present);
    }
    if result.len() != requested.len() { return Err(HistoryErrorCode::InvalidOutput); }
    Ok(result)
}
fn process_error(error: ProcessError) -> HistoryErrorCode {
    if error.cleanup_failed() { return HistoryErrorCode::Inaccessible; }
    match error.failure {
        ProcessFailure::Start(_) => HistoryErrorCode::GitUnavailable,
        ProcessFailure::Io(_) => HistoryErrorCode::Inaccessible,
        ProcessFailure::OutputLimit => HistoryErrorCode::ResourceLimit,
        ProcessFailure::Deadline => HistoryErrorCode::Timeout,
    }
}
fn exit_error(stderr: &[u8]) -> HistoryErrorCode {
    let contains = |needle: &[u8]| stderr.windows(needle.len()).any(|window| window == needle);
    if contains(b"detected dubious ownership") || contains(b"unsafe repository") || contains(b"is owned by someone else") { HistoryErrorCode::UnsafeRepository }
    else if contains(b"bad object") || contains(b"missing") || contains(b"unable to read") || contains(b"not a valid object") { HistoryErrorCode::MissingObjects }
    else { HistoryErrorCode::Inaccessible }
}
