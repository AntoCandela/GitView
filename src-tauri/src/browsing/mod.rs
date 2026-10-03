//! Issues directory-scoped browsing pages and retains opaque authority across refreshes.
//!
//! Paths never arrive from the renderer. A listing retains its issued IDs until refresh-based
//! eviction; traversing more pages never revokes earlier file selections.

use std::{collections::{HashSet, VecDeque}, path::{Path, PathBuf}, sync::Arc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{diff::{self, ReviewErrorCode, ReviewFailure, UnsupportedReason}, git::RepositoryKind,
    history::{reader, HistoryErrorCode}, git::process::{GitProcess, ProbeDeadline}, workspace::SelectedContext};

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod walk;
mod authority;

const MAX_LISTINGS: usize = 8;
const PAGE_ENTRIES: usize = 256;
const MAX_DEPTH: usize = 64;
const PAGE_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_DIRECTORY_CURSORS: usize = 64;

/// All-null starts a listing. Other IDs must have been issued in that listing and selection.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryFilesRequest {
    pub listing_id: Option<String>,
    pub directory_id: Option<String>,
    pub cursor: Option<String>,
}

/// Renderer-safe labels; only the issued ID may authorize a native operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFile { pub id: String, pub display_path: String, pub segments: Vec<String> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDirectory { pub id: String, pub display_path: String, pub segments: Vec<String> }

/// A bounded page. Only a null cursor means this directory, not its descendants, is complete.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositoryFilesResult {
    Files {
        #[serde(rename = "entryId")] entry_id: String,
        #[serde(rename = "listingId")] listing_id: String,
        #[serde(rename = "directoryId")] directory_id: Option<String>,
        files: Vec<RepositoryFile>,
        directories: Vec<RepositoryDirectory>,
        cursor: Option<String>,
    },
    Unavailable { code: HistoryErrorCode, message: &'static str },
    StaleSelection,
}

impl RepositoryFilesResult {
    pub(crate) fn failure(code: HistoryErrorCode) -> Self {
        if code == HistoryErrorCode::StaleSelection { return Self::StaleSelection; }
        let message = match code {
            HistoryErrorCode::GitUnavailable => "Installed Git could not be started.",
            HistoryErrorCode::UnsafeRepository => "Git refused to read this repository because of its ownership.",
            HistoryErrorCode::Timeout => "Repository browsing took too long.",
            HistoryErrorCode::InvalidOutput => "Repository paths or metadata cannot be displayed safely.",
            HistoryErrorCode::ResourceLimit => "Repository browsing reached a native safety or storage limit.",
            HistoryErrorCode::Inaccessible if !cfg!(any(target_os = "macos", target_os = "linux")) =>
                "Safe repository browsing is unavailable on this platform.",
            _ => "Repository files changed or are unavailable at this location. Refresh files to retry.",
        };
        Self::Unavailable { code, message }
    }
}

/// Working contents are read at activation time, not pinned to listing-time file bytes.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositoryFileResult {
    Text {
        #[serde(rename = "entryId")] entry_id: String,
        #[serde(rename = "listingId")] listing_id: String,
        #[serde(rename = "fileId")] file_id: String,
        #[serde(rename = "displayPath")] display_path: String,
        content: String,
    },
    Unsupported { reason: UnsupportedReason },
    Unavailable { code: ReviewErrorCode },
    StaleSelection,
}

impl From<ReviewFailure> for RepositoryFileResult {
    fn from(failure: ReviewFailure) -> Self {
        match failure {
            ReviewFailure::Unsupported(reason) => Self::Unsupported { reason },
            ReviewFailure::Unavailable(code) => Self::Unavailable { code },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeFileKind { Regular, Symlink, Submodule, Other, Directory }
pub(crate) struct NativeFile { file: RepositoryFile, path: PathBuf, kind: NativeFileKind }
struct NativeEntry { file: RepositoryFile, kind: NativeFileKind, identity: Option<(u64, u64)> }
struct Listing {
    id: String,
    context: SelectedContext,
    generation: u64,
    authority: Mutex<authority::Authority>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    cursors: Mutex<std::collections::HashMap<String, Arc<Mutex<DirectoryCursor>>>>,
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
struct DirectoryCursor { directory_id: Option<String>, walk: walk::DirectoryCursor }

pub(crate) struct ListingPage {
    listing: Arc<Listing>, initial: bool, result: RepositoryFilesResult,
    operation: tokio::sync::OwnedMutexGuard<()>,
}

#[derive(Default)]
pub(crate) struct BrowsingController {
    listings: Mutex<VecDeque<Arc<Listing>>>,
    pages: Arc<tokio::sync::Mutex<()>>,
}
impl BrowsingController {
    fn find(&self, context: &SelectedContext, generation: u64, id: &str) -> Option<Arc<Listing>> {
        self.listings.lock().iter().find(|listing| listing.id == id && listing.generation == generation
            && same_context(&listing.context, context)).cloned()
    }

    pub(crate) fn publish(&self, page: ListingPage) -> RepositoryFilesResult {
        let ListingPage { listing, initial, result, operation } = page;
        let mut listings = self.listings.lock();
        let mut retired = Vec::new();
        if initial {
            let mut index = 0;
            while index < listings.len() {
                if same_context(&listings[index].context, &listing.context) && listings[index].generation == listing.generation {
                    index += 1;
                } else { retired.push(listings.remove(index).unwrap()); }
            }
            if listings.len() == MAX_LISTINGS { retired.push(listings.pop_front().unwrap()); }
            listings.push_back(listing);
        } else if !listings.iter().any(|retained| Arc::ptr_eq(retained, &listing)) {
            return RepositoryFilesResult::StaleSelection;
        }
        drop(listings);
        if !retired.is_empty() {
            // Closing SQLite and removing its private files must not run under the
            // application's selection lock. Retain the page permit until cleanup ends.
            tokio::task::spawn_blocking(move || { drop(retired); drop(operation); });
        }
        result
    }

    pub(crate) async fn resolve(&self, context: &SelectedContext, generation: u64, listing_id: &str, file_id: &str, deadline: ProbeDeadline) -> Result<NativeFile, HistoryErrorCode> {
        let listing = self.find(context, generation, listing_id).ok_or(HistoryErrorCode::StaleSelection)?;
        let file_id = file_id.to_owned();
        blocking(deadline, move || {
            let entry = listing.authority.lock().resolve(&file_id)?.ok_or(HistoryErrorCode::StaleSelection)?;
            if entry.kind == NativeFileKind::Directory { return Err(HistoryErrorCode::StaleSelection); }
            Ok(NativeFile { path: PathBuf::from(&entry.file.display_path), file: entry.file, kind: entry.kind })
        }).await
    }

    pub(crate) fn current(&self, context: &SelectedContext, generation: u64, listing_id: &str) -> bool {
        self.find(context, generation, listing_id).is_some()
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(crate) async fn read_page(&self, process: &GitProcess, context: &SelectedContext, generation: u64, request: RepositoryFilesRequest) -> Result<ListingPage, HistoryErrorCode> {
        if context.kind != RepositoryKind::WorkingTree { return Err(HistoryErrorCode::Inaccessible); }
        let deadline = ProbeDeadline::new();
        let operation = tokio::time::timeout_at(deadline.instant(), Arc::clone(&self.pages).lock_owned()).await
            .map_err(|_| HistoryErrorCode::Timeout)?;
        let initial = request.listing_id.is_none();
        let listing = if let Some(id) = &request.listing_id {
            self.find(context, generation, id).ok_or(HistoryErrorCode::StaleSelection)?
        } else {
            if request.directory_id.is_some() || request.cursor.is_some() { return Err(HistoryErrorCode::StaleSelection); }
            let context = copy_context(context);
            blocking(deadline, move || Ok(Arc::new(Listing {
                id: Uuid::new_v4().to_string(), context, generation,
                authority: Mutex::new(authority::Authority::new()?),
                cursors: Mutex::new(std::collections::HashMap::new()),
            }))).await?
        };
        reader::verify_context(process, context, deadline).await?;
        let owned = Arc::clone(&listing);
        let directory_id = request.directory_id.clone();
        let cursor_id = request.cursor.clone();
        let cursor = blocking(deadline, move || {
            if let Some(id) = cursor_id {
                let mut cursors = owned.cursors.lock();
                if !cursors.get(&id).is_some_and(|cursor| cursor.lock().directory_id == directory_id) {
                    return Err(HistoryErrorCode::StaleSelection);
                }
                return Ok(cursors.remove(&id).unwrap());
            }
            if owned.cursors.lock().len() >= MAX_DIRECTORY_CURSORS { return Err(HistoryErrorCode::ResourceLimit); }
            let entry = directory_id.as_deref().map(|id| owned.authority.lock().resolve(id))
                .transpose()?.flatten();
            let (path, identity) = match (directory_id.as_ref(), entry) {
                (None, _) => (PathBuf::new(), None),
                (Some(_), Some(entry)) if entry.kind == NativeFileKind::Directory =>
                    (PathBuf::from(entry.file.display_path), entry.identity),
                _ => return Err(HistoryErrorCode::StaleSelection),
            };
            Ok(Arc::new(Mutex::new(DirectoryCursor { directory_id, walk: walk::DirectoryCursor::open(&owned.context, path, identity, deadline)? })))
        }).await?;
        let position = cursor.lock().walk.position()?;
        let result = read_directory_page(process, &listing, &cursor, deadline).await;
        match result {
            Ok((entries, complete)) => {
                let owned = Arc::clone(&listing);
                let issued = blocking(deadline, move || {
                    owned.authority.lock().issue(&entries, deadline)?;
                    let mut files = Vec::new();
                    let mut directories = Vec::new();
                    for entry in entries {
                        let file = entry.file;
                        if entry.kind == NativeFileKind::Directory {
                            directories.push(RepositoryDirectory { id: file.id, display_path: file.display_path, segments: file.segments });
                        } else { files.push(file); }
                    }
                    Ok((files, directories))
                }).await;
                let (files, directories) = match issued {
                    Ok(issued) => issued,
                    Err(code) => {
                        restore_cursor(&listing, cursor, request.cursor, position, deadline).await;
                        return Err(code);
                    }
                };
                let next = if complete { None } else {
                    let id = Uuid::new_v4().to_string();
                    listing.cursors.lock().insert(id.clone(), cursor);
                    Some(id)
                };
                let result = RepositoryFilesResult::Files {
                    entry_id: context.entry_id.clone(), listing_id: listing.id.clone(),
                    directory_id: request.directory_id, files, directories, cursor: next,
                };
                Ok(ListingPage { listing, initial, result, operation })
            }
            Err(code) => {
                restore_cursor(&listing, cursor, request.cursor, position, deadline).await;
                Err(code)
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(crate) async fn read_page(&self, _process: &GitProcess, _context: &SelectedContext, _generation: u64, _request: RepositoryFilesRequest) -> Result<ListingPage, HistoryErrorCode> {
        Err(HistoryErrorCode::Inaccessible)
    }
}

fn copy_context(context: &SelectedContext) -> SelectedContext {
    SelectedContext { entry_id: context.entry_id.clone(), root: context.root.clone(), git_dir: context.git_dir.clone(),
        identity: context.identity.clone(), kind: context.kind }
}
fn same_context(left: &SelectedContext, right: &SelectedContext) -> bool {
    left.entry_id == right.entry_id && left.root == right.root && left.git_dir == right.git_dir
        && left.kind == right.kind && left.identity == right.identity
}

async fn blocking<T: Send + 'static>(deadline: ProbeDeadline, operation: impl FnOnce() -> Result<T, HistoryErrorCode> + Send + 'static) -> Result<T, HistoryErrorCode> {
    tokio::time::timeout_at(deadline.instant(), tokio::task::spawn_blocking(operation)).await
        .map_err(|_| HistoryErrorCode::Timeout)?.map_err(|_| HistoryErrorCode::Inaccessible)?
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn read_directory_page(process: &GitProcess, listing: &Arc<Listing>, cursor: &Arc<Mutex<DirectoryCursor>>, deadline: ProbeDeadline) -> Result<(Vec<NativeEntry>, bool), HistoryErrorCode> {
    let ancestors = ancestors(cursor.lock().walk.path());
    if has_gitlink_ancestor(process, &listing.context, &ancestors, deadline).await? { return Err(HistoryErrorCode::Inaccessible); }
    let owned = Arc::clone(listing);
    let stream = Arc::clone(cursor);
    let (mut entries, complete) = blocking(deadline, move || stream.lock().walk.page(&owned.context, PageLimits::default(), deadline)).await?;
    let paths = entries.iter().filter(|entry| entry.kind == NativeFileKind::Directory)
        .map(|entry| PathBuf::from(&entry.file.display_path)).collect::<Vec<_>>();
    let links = gitlinks(process, &listing.context, &paths, deadline).await?;
    for entry in &mut entries {
        if links.contains(Path::new(&entry.file.display_path)) { entry.kind = NativeFileKind::Submodule; entry.identity = None; }
    }
    if has_gitlink_ancestor(process, &listing.context, &ancestors, deadline).await? { return Err(HistoryErrorCode::Inaccessible); }
    let owned = Arc::clone(listing);
    let stream = Arc::clone(cursor);
    blocking(deadline, move || stream.lock().walk.validate(&owned.context, deadline)).await?;
    reader::verify_context(process, &listing.context, deadline).await?;
    Ok((entries, complete))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn restore_cursor(listing: &Arc<Listing>, cursor: Arc<Mutex<DirectoryCursor>>, id: Option<String>, position: libc::c_long, deadline: ProbeDeadline) {
    let Some(id) = id else { return; };
    // An expired blocking task still owns its cursor until it stops. Do not wait on its
    // mutex from the async executor; the caller must refresh this explicitly failed page.
    if deadline.check().is_err() { return; }
    let listing = Arc::clone(listing);
    let _ = blocking(deadline, move || {
        cursor.lock().walk.restore(position);
        listing.cursors.lock().insert(id, cursor);
        Ok(())
    }).await;
}

async fn has_gitlink_ancestor(process: &GitProcess, context: &SelectedContext, paths: &[PathBuf], deadline: ProbeDeadline) -> Result<bool, HistoryErrorCode> {
    // Ancestors cannot share a pathspec batch: excluding a parent's descendants would
    // hide a more deeply nested gitlink that is also an explicit target.
    for path in paths {
        if !gitlinks(process, context, std::slice::from_ref(path), deadline).await?.is_empty() { return Ok(true); }
    }
    Ok(false)
}

fn ancestors(path: &Path) -> Vec<PathBuf> {
    path.ancestors().filter(|path| !path.as_os_str().is_empty()).map(Path::to_owned).collect()
}

/// Literal inclusions plus recursive descendant exclusions return only exact index entries.
/// Small batches bound arguments and stdout even for long paths and unmerged index stages.
async fn gitlinks(process: &GitProcess, context: &SelectedContext, paths: &[PathBuf], deadline: ProbeDeadline) -> Result<HashSet<PathBuf>, HistoryErrorCode> {
    let mut links = HashSet::new();
    for batch in paths.chunks(16) {
        // GitProcess forces literal pathspecs for ordinary read operations. Only this
        // native-generated query enables magic; every issued path still uses a literal
        // inclusion and its escaped descendant exclusion.
        let mut arguments = vec!["--no-literal-pathspecs".to_owned(), "ls-files".to_owned(),
            "--stage".to_owned(), "-z".to_owned(), "--".to_owned()];
        for path in batch {
            let path = path.to_str().ok_or(HistoryErrorCode::InvalidOutput)?;
            arguments.push(format!(":(top,literal){path}"));
            let mut escaped = String::with_capacity(path.len());
            for character in path.chars() {
                if matches!(character, '\\' | '*' | '?' | '[' | ']') { escaped.push('\\'); }
                escaped.push(character);
            }
            arguments.push(format!(":(top,glob,exclude){escaped}/**"));
        }
        let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let output = reader::required(process, &context.root, &arguments, None, deadline).await?;
        if output.is_empty() { continue; }
        let body = output.strip_suffix(b"\0").ok_or(HistoryErrorCode::InvalidOutput)?;
        for record in body.split(|byte| *byte == 0) {
            let separator = record.iter().position(|byte| *byte == b'\t').ok_or(HistoryErrorCode::InvalidOutput)?;
            let (metadata, path) = record.split_at(separator);
            let mut fields = metadata.split(|byte| *byte == b' ');
            let mode = fields.next().ok_or(HistoryErrorCode::InvalidOutput)?;
            if !matches!(mode, b"100644" | b"100755" | b"120000" | b"160000") { return Err(HistoryErrorCode::InvalidOutput); }
            reader::oid(fields.next().ok_or(HistoryErrorCode::InvalidOutput)?)?;
            if !matches!(fields.next(), Some(b"0" | b"1" | b"2" | b"3")) || fields.next().is_some() { return Err(HistoryErrorCode::InvalidOutput); }
            let path = PathBuf::from(std::str::from_utf8(&path[1..]).map_err(|_| HistoryErrorCode::InvalidOutput)?);
            if !batch.contains(&path) { return Err(HistoryErrorCode::InvalidOutput); }
            if mode == b"160000" { links.insert(path); }
        }
    }
    Ok(links)
}

pub(crate) async fn read_file(process: &GitProcess, context: &SelectedContext, listing_id: &str, native: NativeFile, deadline: ProbeDeadline) -> RepositoryFileResult {
    let result = async {
        reader::verify_context(process, context, deadline).await.map_err(review_error)?;
        match native.kind {
            NativeFileKind::Regular => (),
            NativeFileKind::Submodule => return Err(diff::unsupported(UnsupportedReason::Submodule)),
            _ => return Err(diff::unsupported(UnsupportedReason::TypeChange)),
        }
        reject_gitlink_ancestors(process, context, &native.path, deadline).await?;
        let bytes = diff::rooted_read::read_browsed(context, &native.path, deadline).await?;
        diff::validate_content(&bytes)?;
        reject_gitlink_ancestors(process, context, &native.path, deadline).await?;
        reader::verify_context(process, context, deadline).await.map_err(review_error)?;
        let content = String::from_utf8(bytes).map_err(|_| diff::unsupported(UnsupportedReason::UnsupportedEncoding))?;
        Ok(RepositoryFileResult::Text {
            entry_id: context.entry_id.clone(), listing_id: listing_id.to_owned(), file_id: native.file.id,
            display_path: native.file.display_path, content,
        })
    }.await;
    result.unwrap_or_else(RepositoryFileResult::from)
}

async fn reject_gitlink_ancestors(process: &GitProcess, context: &SelectedContext, path: &Path, deadline: ProbeDeadline) -> Result<(), ReviewFailure> {
    if has_gitlink_ancestor(process, context, &ancestors(path), deadline).await.map_err(review_error)? {
        return Err(diff::unsupported(UnsupportedReason::Submodule));
    }
    Ok(())
}

pub(crate) fn review_error(code: HistoryErrorCode) -> ReviewFailure {
    diff::unavailable(match code {
        HistoryErrorCode::GitUnavailable => ReviewErrorCode::GitUnavailable,
        HistoryErrorCode::UnsafeRepository => ReviewErrorCode::UnsafeRepository,
        HistoryErrorCode::Timeout => ReviewErrorCode::Timeout,
        HistoryErrorCode::InvalidOutput | HistoryErrorCode::ResourceLimit => ReviewErrorCode::InvalidOutput,
        _ => ReviewErrorCode::ChangedDuringRead,
    })
}

#[derive(Clone, Copy)]
struct PageLimits { entries: usize, depth: usize, output_bytes: usize }
impl Default for PageLimits {
    fn default() -> Self { Self { entries: PAGE_ENTRIES, depth: MAX_DEPTH, output_bytes: PAGE_OUTPUT_BYTES } }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
#[path = "../../tests/integration/browsing.rs"]
mod integration_tests;
