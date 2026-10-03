//! Loads validated workspace choices and replaces their complete private JSON file.
//!
//! Git facts never enter this format. Failed loads do not modify the file; the
//! application owns disabling later saves after a failed or unsupported load.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

const DOCUMENT_LIMIT: usize = 1024 * 1024;
const DOCUMENT_VERSION: u32 = 1;

/// Sanitized storage warnings; neither messages nor codes expose native paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PersistenceError {
    pub code: PersistenceErrorCode,
    pub message: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceErrorCode {
    LoadFailed,
    UnsupportedVersion,
    SaveFailed,
    StorageUnavailable,
}

impl PersistenceError {
    pub(crate) fn load_failed() -> Self {
        Self {
            code: PersistenceErrorCode::LoadFailed,
            message: "Saved workspace could not be read. Current-session navigation still works; saved data is preserved. Repair the workspace file and restart to recover.",
        }
    }

    pub(crate) fn unsupported_version() -> Self {
        Self {
            code: PersistenceErrorCode::UnsupportedVersion,
            message: "Saved workspace uses an unsupported version. Current-session navigation still works; saved data is preserved. Repair the workspace file and restart to recover.",
        }
    }

    pub(crate) fn save_failed() -> Self {
        Self {
            code: PersistenceErrorCode::SaveFailed,
            message: "Workspace choices could not be saved and may not survive restart. Saving will retry after the next successful choice.",
        }
    }

    pub(crate) fn storage_unavailable() -> Self {
        Self {
            code: PersistenceErrorCode::StorageUnavailable,
            message: "Workspace storage is unavailable. Current-session navigation still works; saved data is preserved. Repair application storage and restart to recover.",
        }
    }
}

/// Ordered native locations and nullable selection, never cached repository facts.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceDocument {
    pub version: u32,
    pub repositories: Vec<SavedRepository>,
    // A custom decoder keeps this required even though its value can be null.
    #[serde(deserialize_with = "deserialize_active_root")]
    pub active_root: Option<PathBuf>,
}

impl Default for WorkspaceDocument {
    fn default() -> Self {
        Self {
            version: DOCUMENT_VERSION,
            repositories: Vec::new(),
            active_root: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SavedRepository {
    pub root: PathBuf,
    /// Optional app label; absence derives a label from current Git facts/native root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

fn deserialize_active_root<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<PathBuf>::deserialize(deserializer)
}

/// Missing files are a successful first launch. All other invalid inputs fail whole.
pub(crate) async fn load(path: &Path) -> Result<WorkspaceDocument, PersistenceError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || load_file(&path))
        .await
        .map_err(|_| PersistenceError::load_failed())?
}

fn load_file(path: &Path) -> Result<WorkspaceDocument, PersistenceError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(WorkspaceDocument::default());
        }
        Err(_) => return Err(PersistenceError::load_failed()),
    };
    let mut bytes = Vec::new();
    file.take((DOCUMENT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| PersistenceError::load_failed())?;
    if bytes.len() > DOCUMENT_LIMIT {
        return Err(PersistenceError::load_failed());
    }

    #[derive(Deserialize)]
    struct VersionHeader {
        version: u32,
    }
    // Future schemas need not contain the fields required by version one.
    let header: VersionHeader = serde_json::from_slice(&bytes)
        .map_err(|_| PersistenceError::load_failed())?;
    if header.version != DOCUMENT_VERSION {
        return Err(PersistenceError::unsupported_version());
    }
    let document: WorkspaceDocument = serde_json::from_slice(&bytes)
        .map_err(|_| PersistenceError::load_failed())?;
    if !valid_document(&document) {
        return Err(PersistenceError::load_failed());
    }
    Ok(document)
}

/// Serializes once within the load limit, then syncs a private same-directory
/// temporary file before portable replacement. Failure leaves prior bytes intact.
pub(crate) async fn save(
    path: &Path,
    document: &WorkspaceDocument,
) -> Result<(), PersistenceError> {
    if !valid_document(document) {
        return Err(PersistenceError::save_failed());
    }
    let mut buffer = LimitedBuffer(Vec::new());
    serde_json::to_writer(&mut buffer, document)
        .map_err(|_| PersistenceError::save_failed())?;
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || replace_file(&path, &buffer.0))
        .await
        .map_err(|_| PersistenceError::save_failed())?
}

fn valid_document(document: &WorkspaceDocument) -> bool {
    if document.version != DOCUMENT_VERSION {
        return false;
    }
    let mut roots = HashSet::with_capacity(document.repositories.len());
    for repository in &document.repositories {
        if !valid_root(&repository.root)
            || !roots.insert(repository.root.as_path())
            || repository.display_name.as_ref().is_some_and(|name| name.trim().is_empty())
        {
            return false;
        }
    }
    document.active_root.as_ref().is_none_or(|root| {
        valid_root(root) && roots.contains(root.as_path())
    })
}

fn valid_root(root: &Path) -> bool {
    root.is_absolute()
        && root.to_str().is_some_and(|text| !text.is_empty() && !text.contains('\0'))
}

struct LimitedBuffer(Vec<u8>);

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > DOCUMENT_LIMIT - self.0.len() {
            return Err(io::Error::other("Workspace document exceeds the storage limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|_| PersistenceError::save_failed())?;
    // tempfile defaults to owner-only permissions on Unix and uses native
    // replacement semantics on Windows as well as Unix; never remove first.
    let mut temporary = tempfile::Builder::new()
        .prefix(".gitview-workspace-")
        .tempfile_in(parent)
        .map_err(|_| PersistenceError::save_failed())?;
    temporary.write_all(bytes).map_err(|_| PersistenceError::save_failed())?;
    temporary.flush().map_err(|_| PersistenceError::save_failed())?;
    temporary.as_file().sync_all().map_err(|_| PersistenceError::save_failed())?;
    #[cfg(test)]
    let completed = crate::test_support::before_workspace_persist(path);
    let result = temporary.persist(path).map_err(|_| PersistenceError::save_failed());
    #[cfg(test)]
    if let Some(completed) = completed { let _ = completed.send(()); }
    result.map(|_| ())
}

#[cfg(test)]
#[path = "../../tests/integration/workspace_persistence.rs"]
mod integration_tests;
