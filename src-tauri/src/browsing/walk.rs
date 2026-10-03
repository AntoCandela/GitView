//! Reads bounded directory pages through owned no-follow descriptors.
//!
//! A continuation keeps its original DIR stream and rejects changes to that directory.
//! Descendants are opened only through separately issued directory authority.

use std::{ffi::{CStr, CString}, fs::{File, Metadata}, io::Write,
    os::{fd::{AsRawFd, FromRawFd}, unix::fs::MetadataExt}, path::{Path, PathBuf}, ptr::NonNull};
use super::{NativeEntry, NativeFileKind, PageLimits, RepositoryFile};
use crate::{diff::{self, ReviewErrorCode, ReviewFailure}, history::HistoryErrorCode,
    git::process::ProbeDeadline, workspace::SelectedContext};

pub(super) struct DirectoryCursor {
    directory: File,
    stream: DirectoryStream,
    path: PathBuf,
    fingerprint: Fingerprint,
}

impl DirectoryCursor {
    pub(super) fn open(context: &SelectedContext, path: PathBuf, identity: Option<(u64, u64)>, deadline: ProbeDeadline) -> Result<Self, HistoryErrorCode> {
        let directory = open_path(context, &path, deadline)?;
        let fingerprint = fingerprint(&directory)?;
        if identity.is_some_and(|identity| identity != (fingerprint.device, fingerprint.inode)) {
            return Err(HistoryErrorCode::Inaccessible);
        }
        let stream = DirectoryStream::new(&directory)?;
        Ok(Self { directory, stream, path, fingerprint })
    }

    pub(super) fn path(&self) -> &Path { &self.path }
    pub(super) fn position(&self) -> Result<libc::c_long, HistoryErrorCode> { self.stream.position() }
    pub(super) fn restore(&mut self, position: libc::c_long) { self.stream.restore(position); }

    pub(super) fn validate(&self, context: &SelectedContext, deadline: ProbeDeadline) -> Result<(), HistoryErrorCode> {
        deadline.check().map_err(|_| HistoryErrorCode::Timeout)?;
        let current = open_path(context, &self.path, deadline)?;
        if self.fingerprint != fingerprint(&self.directory)? || self.fingerprint != fingerprint(&current)? {
            return Err(HistoryErrorCode::Inaccessible);
        }
        Ok(())
    }

    pub(super) fn page(&mut self, context: &SelectedContext, limits: PageLimits, deadline: ProbeDeadline) -> Result<(Vec<NativeEntry>, bool), HistoryErrorCode> {
        self.validate(context, deadline)?;
        if limits.entries == 0 { return Err(HistoryErrorCode::ResourceLimit); }
        let metadata_path = context.git_dir.strip_prefix(&context.root).ok();
        let depth = self.path.components().count();
        let mut entries = Vec::new();
        let mut output = OutputBudget { bytes: 512 + context.entry_id.len(), limit: limits.output_bytes };
        if output.bytes > output.limit { return Err(HistoryErrorCode::ResourceLimit); }
        let mut complete = false;
        for _ in 0..limits.entries {
            deadline.check().map_err(|_| HistoryErrorCode::Timeout)?;
            let position = self.stream.position()?;
            let Some(name) = self.stream.next()? else { complete = true; break; };
            if matches!(name.to_bytes(), b"." | b".." | b".git") { continue; }
            let name_text = name.to_str().map_err(|_| HistoryErrorCode::InvalidOutput)?;
            let path = self.path.join(name_text);
            if metadata_path.is_some_and(|metadata| path == metadata || path.starts_with(metadata)) { continue; }
            if depth + 1 > limits.depth { return Err(HistoryErrorCode::ResourceLimit); }
            let metadata = entry_metadata(&self.directory, name)?;
            let mode = metadata.st_mode & libc::S_IFMT;
            let (kind, identity) = if mode == libc::S_IFDIR {
                let child = open_directory(&self.directory, name, &metadata)?;
                if diff::rooted_read::is_nested_repository(&child, deadline).map_err(read_error)? {
                    (NativeFileKind::Submodule, None)
                } else {
                    (NativeFileKind::Directory, Some((metadata.st_dev as u64, metadata.st_ino as u64)))
                }
            } else {
                (if mode == libc::S_IFREG { NativeFileKind::Regular }
                    else if mode == libc::S_IFLNK { NativeFileKind::Symlink } else { NativeFileKind::Other }, None)
            };
            let display_path = path.to_str().ok_or(HistoryErrorCode::InvalidOutput)?.to_owned();
            let segments = display_path.split('/').map(str::to_owned).collect();
            let file = RepositoryFile { id: uuid::Uuid::new_v4().to_string(), display_path, segments };
            if serde_json::to_writer(&mut output, &file).and_then(|_| output.write_all(b",").map_err(serde_json::Error::io)).is_err() {
                if entries.is_empty() { return Err(HistoryErrorCode::ResourceLimit); }
                self.stream.restore(position);
                break;
            }
            entries.push(NativeEntry { file, kind, identity });
        }
        self.validate(context, deadline)?;
        Ok((entries, complete))
    }
}

#[derive(PartialEq, Eq)]
struct Fingerprint { device: u64, inode: u64, modified: (i64, i64), changed: (i64, i64) }
fn fingerprint(file: &File) -> Result<Fingerprint, HistoryErrorCode> {
    let metadata: Metadata = file.metadata().map_err(|_| HistoryErrorCode::Inaccessible)?;
    Ok(Fingerprint { device: metadata.dev(), inode: metadata.ino(), modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()) })
}

fn open_path(context: &SelectedContext, path: &Path, deadline: ProbeDeadline) -> Result<File, HistoryErrorCode> {
    let mut chain = diff::rooted_read::open_root(context, deadline).map_err(read_error)?;
    for component in path.components() {
        deadline.check().map_err(|_| HistoryErrorCode::Timeout)?;
        let std::path::Component::Normal(name) = component else { return Err(HistoryErrorCode::InvalidOutput); };
        let name = CString::new(name.as_encoded_bytes()).map_err(|_| HistoryErrorCode::InvalidOutput)?;
        let parent = chain.last().ok_or(HistoryErrorCode::Inaccessible)?;
        let metadata = entry_metadata(parent, &name)?;
        let directory = open_directory(parent, &name, &metadata)?;
        if diff::rooted_read::is_nested_repository(&directory, deadline).map_err(read_error)? {
            return Err(HistoryErrorCode::Inaccessible);
        }
        chain.push(directory);
    }
    chain.pop().ok_or(HistoryErrorCode::Inaccessible)
}

struct OutputBudget { bytes: usize, limit: usize }
impl Write for OutputBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.checked_add(bytes.len()).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::OutOfMemory))?;
        if self.bytes > self.limit { return Err(std::io::Error::from(std::io::ErrorKind::OutOfMemory)); }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

struct DirectoryStream(NonNull<libc::DIR>);
// SAFETY: a DIR is uniquely owned and never accessed concurrently. Moving it between blocking
// threads is safe; all access is serialized by the containing cursor's mutex.
unsafe impl Send for DirectoryStream {}
impl DirectoryStream {
    fn new(directory: &File) -> Result<Self, HistoryErrorCode> {
        // SAFETY: fcntl duplicates a live borrowed descriptor; fdopendir takes the duplicate only.
        let descriptor = unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if descriptor < 0 { return Err(HistoryErrorCode::Inaccessible); }
        let stream = unsafe { libc::fdopendir(descriptor) };
        match NonNull::new(stream) {
            Some(stream) => Ok(Self(stream)),
            None => {
                // SAFETY: failed fdopendir leaves its supplied duplicate owned by this function.
                unsafe { libc::close(descriptor); }
                Err(HistoryErrorCode::Inaccessible)
            }
        }
    }

    fn position(&self) -> Result<libc::c_long, HistoryErrorCode> {
        // SAFETY: the stream is live and exclusively accessed by the cursor owner.
        let position = unsafe { libc::telldir(self.0.as_ptr()) };
        if position < 0 { Err(HistoryErrorCode::Inaccessible) } else { Ok(position) }
    }
    fn restore(&mut self, position: libc::c_long) {
        // SAFETY: this cookie was obtained from this same still-open, unchanged stream.
        unsafe { libc::seekdir(self.0.as_ptr(), position); }
    }

    fn next(&mut self) -> Result<Option<&CStr>, HistoryErrorCode> {
        #[cfg(target_os = "macos")]
        let errno = unsafe { libc::__error() };
        #[cfg(target_os = "linux")]
        let errno = unsafe { libc::__errno_location() };
        // SAFETY: the stream is uniquely owned and errno belongs to this blocking thread.
        unsafe {
            *errno = 0;
            let entry = libc::readdir(self.0.as_ptr());
            if entry.is_null() {
                return if *errno == 0 { Ok(None) } else { Err(HistoryErrorCode::Inaccessible) };
            }
            Ok(Some(CStr::from_ptr((*entry).d_name.as_ptr())))
        }
    }
}
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this unique stream owns its descriptor and has not been closed elsewhere.
        unsafe { libc::closedir(self.0.as_ptr()); }
    }
}

fn entry_metadata(directory: &File, name: &CStr) -> Result<libc::stat, HistoryErrorCode> {
    let mut metadata = std::mem::MaybeUninit::uninit();
    // SAFETY: live parent descriptor, NUL-terminated name and writable stat storage; no links followed.
    let result = unsafe { libc::fstatat(directory.as_raw_fd(), name.as_ptr(), metadata.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
    if result != 0 { return Err(HistoryErrorCode::Inaccessible); }
    // SAFETY: successful fstatat initialized every stat field.
    Ok(unsafe { metadata.assume_init() })
}

fn open_directory(parent: &File, name: &CStr, expected: &libc::stat) -> Result<File, HistoryErrorCode> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: the borrowed parent is live and name is NUL terminated; successful fd is newly owned.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 { return Err(HistoryErrorCode::Inaccessible); }
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(|_| HistoryErrorCode::Inaccessible)?;
    if metadata.dev() != expected.st_dev as u64 || metadata.ino() != expected.st_ino as u64 { return Err(HistoryErrorCode::Inaccessible); }
    Ok(file)
}

fn read_error(failure: ReviewFailure) -> HistoryErrorCode {
    match failure {
        ReviewFailure::Unavailable(ReviewErrorCode::Timeout) => HistoryErrorCode::Timeout,
        ReviewFailure::Unavailable(ReviewErrorCode::InvalidOutput) => HistoryErrorCode::InvalidOutput,
        _ => HistoryErrorCode::Inaccessible,
    }
}
