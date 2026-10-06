//! Persists only opt-in intent; unreadable or future documents stay protected for the session.

use std::{fs::{self, File}, io::{self, Read, Write}, path::PathBuf};
use serde::{Deserialize, Serialize};
use super::PersistenceError;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    enabled: bool,
}

pub(super) struct Preferences {
    path: Option<PathBuf>,
    protected: bool,
    pub enabled: bool,
    pub error: Option<PersistenceError>,
}

impl Preferences {
    pub fn unsupported() -> Self {
        Self { path: None, protected: true, enabled: false, error: None }
    }

    pub fn load(path: Option<PathBuf>) -> Self {
        let result = path.as_ref().ok_or(PersistenceError::StorageUnavailable)
            .and_then(|path| Self::read(path));
        match result {
            Ok(enabled) => Self { path, protected: false, enabled, error: None },
            Err(error) => Self { path, protected: true, enabled: false, error: Some(error) },
        }
    }

    fn read(path: &PathBuf) -> Result<bool, PersistenceError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(PersistenceError::LoadFailed),
        };
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes).map_err(|_| PersistenceError::LoadFailed)?;
        if bytes.len() > 4096 { return Err(PersistenceError::LoadFailed); }
        #[derive(Deserialize)]
        struct Header { version: u32 }
        let header: Header = serde_json::from_slice(&bytes).map_err(|_| PersistenceError::LoadFailed)?;
        if header.version != 1 { return Err(PersistenceError::UnsupportedVersion); }
        let document: Document = serde_json::from_slice(&bytes).map_err(|_| PersistenceError::LoadFailed)?;
        Ok(document.enabled)
    }

    pub fn save(&mut self, enabled: bool) {
        self.enabled = enabled;
        if self.protected { return; }
        self.error = self.replace(enabled).err();
    }

    fn replace(&self, enabled: bool) -> Result<(), PersistenceError> {
        let path = self.path.as_ref().ok_or(PersistenceError::StorageUnavailable)?;
        let parent = path.parent().ok_or(PersistenceError::SaveFailed)?;
        fs::create_dir_all(parent).map_err(|_| PersistenceError::SaveFailed)?;
        let mut temporary = tempfile::Builder::new().prefix(".gitview-companion-")
            .tempfile_in(parent).map_err(|_| PersistenceError::SaveFailed)?;
        serde_json::to_writer(&mut temporary, &Document { version: 1, enabled })
            .map_err(|_| PersistenceError::SaveFailed)?;
        temporary.flush().map_err(|_| PersistenceError::SaveFailed)?;
        temporary.as_file().sync_all().map_err(|_| PersistenceError::SaveFailed)?;
        temporary.persist(path).map_err(|_| PersistenceError::SaveFailed)?;
        Ok(())
    }
}
