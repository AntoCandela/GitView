//! Reads regular worktree files through no-follow descriptors and rejects identity races.

use std::path::Path;
use super::{ReviewFailure, ReviewErrorCode, UnsupportedReason, unavailable, unsupported};
use crate::git::process::ProbeDeadline;
use crate::workspace::SelectedContext;

pub(crate) async fn read(context: &SelectedContext, path: &Path, deadline: ProbeDeadline) -> Result<Vec<u8>, ReviewFailure> {
    read_with_policy(context, path, deadline, false).await
}

/// Browsing must not traverse a directory that became a nested repository after listing.
pub(crate) async fn read_browsed(context: &SelectedContext, path: &Path, deadline: ProbeDeadline) -> Result<Vec<u8>, ReviewFailure> {
    read_with_policy(context, path, deadline, true).await
}

async fn read_with_policy(context: &SelectedContext, path: &Path, deadline: ProbeDeadline, reject_nested_git: bool) -> Result<Vec<u8>, ReviewFailure> {
    #[cfg(unix)] {
        let root = context.root.clone();
        let expected = context.identity.root.clone();
        let path = path.to_owned();
        let task = tokio::task::spawn_blocking(move || read_blocking(&root, &path, &expected, deadline, reject_nested_git));
        tokio::time::timeout_at(deadline.instant(), task).await
            .map_err(|_| unavailable(ReviewErrorCode::Timeout))?
            .map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?
    }
    #[cfg(not(unix))] {
        let _ = (context, path, deadline, reject_nested_git);
        Err(unsupported(UnsupportedReason::Other))
    }
}

#[cfg(unix)]
pub(crate) fn open_root(context: &SelectedContext, deadline: ProbeDeadline) -> Result<Vec<std::fs::File>, ReviewFailure> {
    open_chain(&context.root, Path::new(""), &context.identity.root, deadline, false, false)
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
struct Fingerprint { device: u64, inode: u64, mode: u32, size: u64, modified: (i64, i64), changed: (i64, i64) }
#[cfg(unix)]
fn fingerprint(file: &std::fs::File) -> Result<Fingerprint, ReviewFailure> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
    // Unrelated child creation must not invalidate a directory's rooted identity.
    let directory = metadata.is_dir();
    Ok(Fingerprint { device: metadata.dev(), inode: metadata.ino(), mode: metadata.mode(),
        size: if directory { 0 } else { metadata.len() },
        modified: if directory { (0, 0) } else { (metadata.mtime(), metadata.mtime_nsec()) },
        changed: if directory { (0, 0) } else { (metadata.ctime(), metadata.ctime_nsec()) } })
}

#[cfg(unix)]
fn open_chain(root: &Path, path: &Path, expected: &crate::workspace::DirectoryIdentity, deadline: ProbeDeadline, leaf_file: bool, reject_nested_git: bool) -> Result<Vec<std::fs::File>, ReviewFailure> {
    use std::ffi::CString;
    use std::os::{fd::{AsRawFd, FromRawFd}, unix::ffi::OsStrExt};
    use std::path::Component;
    if !root.is_absolute() || path.components().any(|part| !matches!(part, Component::Normal(_))) || (leaf_file && path.as_os_str().is_empty()) {
        return Err(unavailable(ReviewErrorCode::InvalidOutput));
    }
    let mut options = std::fs::OpenOptions::new();
    use std::os::unix::fs::OpenOptionsExt;
    options.read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut chain = vec![options.open("/").map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?];
    if root == Path::new("/") {
        let identity = fingerprint(&chain[0])?;
        if identity.device != expected.device || identity.inode != expected.inode {
            return Err(unavailable(ReviewErrorCode::ChangedDuringRead));
        }
    }
    let components: Vec<_> = root.components().filter_map(|part| match part { Component::Normal(name) => Some(name), _ => None })
        .chain(path.components().filter_map(|part| match part { Component::Normal(name) => Some(name), _ => None })).collect();
    let root_depth = root.components().filter(|part| matches!(part, Component::Normal(_))).count();
    for (index, component) in components.iter().enumerate() {
        deadline.check().map_err(super::process_error)?;
        let name = CString::new(component.as_bytes()).map_err(|_| unavailable(ReviewErrorCode::InvalidOutput))?;
        let is_file = leaf_file && index + 1 == components.len();
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK | if is_file { 0 } else { libc::O_DIRECTORY };
        // SAFETY: the parent descriptor remains owned in chain, name is NUL terminated,
        // and a successful returned descriptor is immediately transferred to File.
        let descriptor = unsafe { libc::openat(chain.last().unwrap().as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            let error = std::io::Error::last_os_error();
            return Err(if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR | libc::ENOENT)) {
                unavailable(ReviewErrorCode::ChangedDuringRead)
            } else { unavailable(ReviewErrorCode::Inaccessible) });
        }
        // SAFETY: openat returned a new owned descriptor, not shared with another File.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let metadata = file.metadata().map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
        if is_file && !metadata.is_file() { return Err(unsupported(UnsupportedReason::TypeChange)); }
        if reject_nested_git && !is_file && index + 1 > root_depth && is_nested_repository(&file, deadline)? {
            return Err(unsupported(UnsupportedReason::Submodule));
        }
        chain.push(file);
        if index + 1 == root_depth {
            let root_fingerprint = fingerprint(chain.last().unwrap())?;
            if root_fingerprint.device != expected.device || root_fingerprint.inode != expected.inode {
                return Err(unavailable(ReviewErrorCode::ChangedDuringRead));
            }
        }
    }
    Ok(chain)
}

/// Recognizes nested worktrees and Git's bare-directory signature without following links.
#[cfg(unix)]
pub(crate) fn is_nested_repository(directory: &std::fs::File, deadline: ProbeDeadline) -> Result<bool, ReviewFailure> {
    deadline.check().map_err(super::process_error)?;
    if entry_mode(directory, c".git")?.is_some() { return Ok(true); }
    let directory_or_link = |mode| matches!(mode, Some(libc::S_IFDIR | libc::S_IFLNK));
    if !directory_or_link(entry_mode(directory, c"objects")?) || !directory_or_link(entry_mode(directory, c"refs")?) {
        return Ok(false);
    }
    match entry_mode(directory, c"HEAD")? {
        Some(libc::S_IFLNK) => Ok(true),
        Some(libc::S_IFREG) => valid_bare_head(directory, deadline),
        _ => Ok(false),
    }
}

#[cfg(unix)]
fn entry_mode(directory: &std::fs::File, name: &std::ffi::CStr) -> Result<Option<libc::mode_t>, ReviewFailure> {
    use std::os::fd::AsRawFd;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: live descriptor, NUL-terminated name and writable storage; no link is followed.
    let result = unsafe { libc::fstatat(directory.as_raw_fd(), name.as_ptr(), metadata.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
    if result == 0 {
        // SAFETY: successful fstatat initialized the stat structure.
        return Ok(Some(unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT));
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) { return Ok(None); }
    Err(unavailable(ReviewErrorCode::Inaccessible))
}

#[cfg(unix)]
fn valid_bare_head(directory: &std::fs::File, deadline: ProbeDeadline) -> Result<bool, ReviewFailure> {
    use std::{io::Read, os::fd::{AsRawFd, FromRawFd}};
    const HEAD_LIMIT: usize = 4096;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    // SAFETY: the borrowed directory is live and HEAD is a fixed NUL-terminated name.
    let descriptor = unsafe { libc::openat(directory.as_raw_fd(), c"HEAD".as_ptr(), flags) };
    if descriptor < 0 { return Err(unavailable(ReviewErrorCode::ChangedDuringRead)); }
    // SAFETY: successful openat returned a new owned descriptor.
    let mut file = unsafe { std::fs::File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
    if !metadata.is_file() { return Err(unavailable(ReviewErrorCode::ChangedDuringRead)); }
    if metadata.len() > HEAD_LIMIT as u64 { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
    let before = fingerprint(&file)?;
    let mut buffer = [0u8; HEAD_LIMIT + 1];
    let mut length = 0;
    while length < buffer.len() {
        deadline.check().map_err(super::process_error)?;
        let count = file.read(&mut buffer[length..]).map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
        if count == 0 { break; }
        length += count;
    }
    if length > HEAD_LIMIT { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
    if before != fingerprint(&file)? || length as u64 != metadata.len() {
        return Err(unavailable(ReviewErrorCode::ChangedDuringRead));
    }
    let head = std::str::from_utf8(&buffer[..length]).map_err(|_| unsupported(UnsupportedReason::UnsupportedEncoding))?
        .trim_matches(|character: char| character.is_ascii_whitespace());
    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim_start_matches(|character: char| character.is_ascii_whitespace());
        return Ok(reference.starts_with("refs/") && crate::history::reader::valid_ref_name(reference));
    }
    Ok(matches!(head.len(), 40 | 64) && head.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(unix)]
fn read_blocking(root: &Path, path: &Path, expected: &crate::workspace::DirectoryIdentity, deadline: ProbeDeadline, reject_nested_git: bool) -> Result<Vec<u8>, ReviewFailure> {
    use std::io::Read;
    let mut chain = open_chain(root, path, expected, deadline, true, reject_nested_git)?;
    let before = chain.iter().map(fingerprint).collect::<Result<Vec<_>, _>>()?;
    if before.last().unwrap().size > super::CONTENT_LIMIT as u64 { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
    let mut bytes = Vec::with_capacity(before.last().unwrap().size as usize);
    let mut chunk = [0u8; 8192];
    loop {
        deadline.check().map_err(super::process_error)?;
        let read = chain.last_mut().unwrap().read(&mut chunk).map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
        if read == 0 { break; }
        if read > super::CONTENT_LIMIT.saturating_sub(bytes.len()) { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let after = chain.iter().map(fingerprint).collect::<Result<Vec<_>, _>>()?;
    let reopened = open_chain(root, path, expected, deadline, true, reject_nested_git)?;
    let current = reopened.iter().map(fingerprint).collect::<Result<Vec<_>, _>>()?;
    if before != after || after != current || bytes.len() as u64 != before.last().unwrap().size {
        return Err(unavailable(ReviewErrorCode::ChangedDuringRead));
    }
    Ok(bytes)
}
