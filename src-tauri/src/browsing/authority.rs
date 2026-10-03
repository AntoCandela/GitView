//! Keeps issued browsing authority off the heap in a private, disposable SQLite store.
//!
//! Each of eight retained listings has a 256 KiB page cache and a 256 MiB storage ceiling.
//! Reaching the ceiling fails the page transaction; it never evicts already issued IDs.

use rusqlite::{params, Connection, OptionalExtension};
use super::{NativeEntry, NativeFileKind, RepositoryFile};
use crate::{history::HistoryErrorCode, git::process::ProbeDeadline};

pub(super) struct Authority {
    // Field order closes SQLite before its private directory is removed.
    connection: Connection,
    _directory: tempfile::TempDir,
}

impl Authority {
    pub(super) fn new() -> Result<Self, HistoryErrorCode> {
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = builder.tempdir().map_err(|_| HistoryErrorCode::Inaccessible)?;
        let path = directory.path().join("authority.sqlite");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path).map_err(|_| HistoryErrorCode::Inaccessible)?;
        let connection = Connection::open(path).map_err(database_error)?;
        connection.execute_batch("PRAGMA page_size=4096;
            PRAGMA max_page_count=65536;
            PRAGMA cache_size=-256;
            PRAGMA mmap_size=0;
            PRAGMA journal_mode=MEMORY;
            PRAGMA temp_store=MEMORY;
            CREATE TABLE entries (
                id TEXT PRIMARY KEY, path TEXT NOT NULL, kind INTEGER NOT NULL,
                device INTEGER, inode INTEGER
            ) WITHOUT ROWID;").map_err(database_error)?;
        Ok(Self { connection, _directory: directory })
    }

    pub(super) fn issue(&mut self, entries: &[NativeEntry], deadline: ProbeDeadline) -> Result<(), HistoryErrorCode> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        {
            let mut insert = transaction.prepare_cached("INSERT INTO entries (id, path, kind, device, inode) VALUES (?1, ?2, ?3, ?4, ?5)")
                .map_err(database_error)?;
            for entry in entries {
                deadline.check().map_err(|_| HistoryErrorCode::Timeout)?;
                let kind = match entry.kind {
                    NativeFileKind::Regular => 0, NativeFileKind::Symlink => 1,
                    NativeFileKind::Submodule => 2, NativeFileKind::Other => 3, NativeFileKind::Directory => 4,
                };
                insert.execute(params![entry.file.id, entry.file.display_path, kind,
                    entry.identity.map(|identity| identity.0 as i64), entry.identity.map(|identity| identity.1 as i64)])
                    .map_err(database_error)?;
            }
        }
        deadline.check().map_err(|_| HistoryErrorCode::Timeout)?;
        transaction.commit().map_err(database_error)
    }

    pub(super) fn resolve(&self, id: &str) -> Result<Option<NativeEntry>, HistoryErrorCode> {
        // UUID parsing bounds untrusted request input before it reaches SQLite.
        if uuid::Uuid::parse_str(id).is_err() { return Ok(None); }
        self.connection.query_row("SELECT path, kind, device, inode FROM entries WHERE id=?1", [id], |row| {
            let display_path: String = row.get(0)?;
            let kind = match row.get::<_, i64>(1)? {
                0 => NativeFileKind::Regular, 1 => NativeFileKind::Symlink, 2 => NativeFileKind::Submodule,
                3 => NativeFileKind::Other, 4 => NativeFileKind::Directory,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let device: Option<i64> = row.get(2)?;
            let inode: Option<i64> = row.get(3)?;
            let segments = display_path.split('/').map(str::to_owned).collect();
            Ok(NativeEntry { file: RepositoryFile { id: id.to_owned(), display_path, segments }, kind,
                identity: device.zip(inode).map(|(device, inode)| (device as u64, inode as u64)) })
        }).optional().map_err(database_error)
    }
}

fn database_error(error: rusqlite::Error) -> HistoryErrorCode {
    match error {
        rusqlite::Error::SqliteFailure(error, _) if error.code == rusqlite::ErrorCode::DiskFull => HistoryErrorCode::ResourceLimit,
        _ => HistoryErrorCode::Inaccessible,
    }
}

#[cfg(test)]
#[path = "../../tests/integration/browsing_authority.rs"]
mod integration_tests;
