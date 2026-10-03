//! Validates stores before any mutating pragma and rejects unsafe filesystem destinations.

use super::Code;
use rusqlite::{ffi, params_from_iter, Connection, OpenFlags};
use serde::Serialize;
use std::{fs, io::{Read, Seek, SeekFrom, Write}, path::{Component, Path, PathBuf}, time::Duration};

pub(super) const VERSION: u32 = 1;
const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_JOURNAL_BYTES: u64 = 2 * MAX_DATABASE_BYTES;
const JOURNAL_MAGIC: [u8; 8] = [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7];
pub(super) const TABLE: &str = "CREATE TABLE events(id INTEGER PRIMARY KEY, timestamp_ms INTEGER NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, parent_operation_id TEXT, operation_kind TEXT NOT NULL DEFAULT 'unknown', level TEXT NOT NULL CHECK(level IN ('debug','info','warn','error')), component TEXT NOT NULL, event TEXT NOT NULL, code TEXT, duration_ms INTEGER, exit_code INTEGER, stdout_bytes INTEGER, stderr_bytes INTEGER, cleanup_failed INTEGER NOT NULL DEFAULT 0)";
pub(super) const INDEXES: [(&str, &str); 6] = [
    ("events_timestamp", "CREATE INDEX events_timestamp ON events(timestamp_ms,id)"),
    ("events_level", "CREATE INDEX events_level ON events(level,timestamp_ms,id)"),
    ("events_operation", "CREATE INDEX events_operation ON events(operation_id,id)"),
    ("events_session", "CREATE INDEX events_session ON events(session_id,id)"),
    ("events_component", "CREATE INDEX events_component ON events(component,timestamp_ms,id)"),
    ("events_event", "CREATE INDEX events_event ON events(event,timestamp_ms,id)"),
];

#[derive(Debug, Serialize)]
pub struct SchemaColumn { pub name: String, pub data_type: String, pub not_null: bool, pub primary_key: bool }
#[derive(Debug, Serialize)]
pub struct SchemaIndex { pub name: String, pub sql: String }
#[derive(Debug, Serialize)]
pub struct SchemaReport {
    pub version: u32,
    pub sql: String,
    pub columns: Vec<SchemaColumn>,
    pub indexes: Vec<SchemaIndex>,
}

pub(super) fn read_connection(path: &Path) -> Result<Connection, Code> {
    let connection = open_read_connection(path)?;
    validate(&connection)?;
    Ok(connection)
}

/// Only a proven rollback requirement admits private-snapshot validation and source recovery.
/// Returns a recovered writable connection, or `None` when ordinary read-only validation succeeds.
pub(super) fn capture_connection(path: &Path) -> Result<Option<Connection>, Code> {
    let connection = open_read_connection(path)?;
    match validate(&connection) {
        Ok(()) => Ok(None),
        Err(code) => {
            // The connection is live and confined to this thread; inspect before another SQLite call.
            let error = unsafe { ffi::sqlite3_extended_errcode(connection.handle()) };
            if error != ffi::SQLITE_READONLY_ROLLBACK { return Err(code); }
            drop(connection);
            recover_connection(path).map(Some)
        },
    }
}

fn open_read_connection(path: &Path) -> Result<Connection, Code> {
    reject_symlinks(path)?;
    reject_sidecars(path)?;
    let metadata = fs::metadata(path).map_err(|_| Code::StorageUnavailable)?;
    if !metadata.is_file() || metadata.len() > MAX_DATABASE_BYTES { return Err(Code::StorageUnavailable); }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW)
        .map_err(|_| Code::StorageUnavailable)?;
    connection.busy_timeout(Duration::from_millis(100)).map_err(|_| Code::Storage)?;
    connection.pragma_update(None, "query_only", true).map_err(|_| Code::Storage)?;
    Ok(connection)
}

fn recover_connection(path: &Path) -> Result<Connection, Code> {
    reject_symlinks(path)?;
    reject_sidecars(path)?;
    let directory = path.parent().ok_or(Code::StorageUnavailable)?;
    require_owned_private(&fs::metadata(directory).map_err(|_| Code::StorageUnavailable)?)?;
    let database_before = recovery_metadata(path, MAX_DATABASE_BYTES)?;
    let journal = sidecar_path(path, "-journal");
    let journal_before = recovery_metadata(&journal, MAX_JOURNAL_BYTES)?;
    for suffix in ["-wal", "-shm"] {
        if fs::symlink_metadata(sidecar_path(path, suffix)).is_ok() { return Err(Code::Schema); }
    }
    // Opening without CREATE or a database-reading statement does not invoke the pager's recovery.
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW)
        .map_err(|_| Code::StorageUnavailable)?;
    let file = database_file(&connection)?;
    lock_snapshot(file)?;
    require_unchanged(path, &database_before)?;
    require_database_not_moved(&connection)?;

    let snapshot = tempfile::Builder::new().prefix(".gitview-recovery-").tempdir_in(directory).map_err(|_| Code::StorageUnavailable)?;
    let snapshot_path = snapshot.path().join("diagnostics.sqlite");
    copy_database(file, database_before.len(), &snapshot_path)?;
    copy_journal(&journal, &journal_before, &sidecar_path(&snapshot_path, "-journal"))?;
    let recovered = Connection::open_with_flags(&snapshot_path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW)
        .map_err(|_| Code::StorageUnavailable)?;
    validate(&recovered)?;
    let integrity: String = recovered.query_row("PRAGMA quick_check(1)", [], |row| row.get(0)).map_err(|_| Code::Schema)?;
    if integrity != "ok" || sidecar_path(&snapshot_path, "-journal").exists() { return Err(Code::Schema); }
    drop(recovered);
    require_unchanged(path, &database_before)?;
    require_unchanged(&journal, &journal_before)?;
    reject_symlinks(path)?;
    reject_sidecars(path)?;
    require_database_not_moved(&connection)?;

    // A RESERVED-or-stronger VFS lock would hide the hot journal from the pager.
    // Keep SHARED instead: no writer can recover this journal or commit while it is held.
    // The same handle's first read then upgrades to EXCLUSIVE and performs SQLite's rollback.
    let unlock = unsafe { (*(*file).pMethods).xUnlock }.ok_or(Code::StorageUnavailable)?;
    if unsafe { unlock(file, ffi::SQLITE_LOCK_SHARED) } != ffi::SQLITE_OK { return Err(Code::StorageUnavailable); }
    validate(&connection)?;
    Ok(connection)
}

fn database_file(connection: &Connection) -> Result<*mut ffi::sqlite3_file, Code> {
    let mut file: *mut ffi::sqlite3_file = std::ptr::null_mut();
    // SQLite owns the returned file for the lifetime of this thread-confined connection.
    let result = unsafe { ffi::sqlite3_file_control(connection.handle(), c"main".as_ptr(), ffi::SQLITE_FCNTL_FILE_POINTER, (&mut file as *mut *mut ffi::sqlite3_file).cast()) };
    if result != ffi::SQLITE_OK || file.is_null() || unsafe { (*file).pMethods.is_null() } { return Err(Code::StorageUnavailable); }
    Ok(file)
}

fn lock_snapshot(file: *mut ffi::sqlite3_file) -> Result<(), Code> {
    // Acquire through the bundled VFS, not separate OS descriptors whose close can drop Unix locks.
    // Connection drop releases even a partially acquired lock without reading the source database.
    let lock = unsafe { (*(*file).pMethods).xLock }.ok_or(Code::StorageUnavailable)?;
    if unsafe { lock(file, ffi::SQLITE_LOCK_SHARED) } != ffi::SQLITE_OK
        || unsafe { lock(file, ffi::SQLITE_LOCK_EXCLUSIVE) } != ffi::SQLITE_OK {
        return Err(Code::StorageUnavailable);
    }
    Ok(())
}

fn require_database_not_moved(connection: &Connection) -> Result<(), Code> {
    let mut moved: i32 = 1;
    // Unix checks pathname/inode identity. Windows' bundled VFS denies delete-sharing on open files.
    let result = unsafe { ffi::sqlite3_file_control(connection.handle(), c"main".as_ptr(), ffi::SQLITE_FCNTL_HAS_MOVED, (&mut moved as *mut i32).cast()) };
    #[cfg(windows)]
    if result == ffi::SQLITE_NOTFOUND { return Ok(()); }
    if result != ffi::SQLITE_OK || moved != 0 { return Err(Code::StorageUnavailable); }
    Ok(())
}

fn copy_database(file: *mut ffi::sqlite3_file, length: u64, destination: &Path) -> Result<(), Code> {
    let mut output = private_file(destination)?;
    let read = unsafe { (*(*file).pMethods).xRead }.ok_or(Code::StorageUnavailable)?;
    let mut buffer = [0_u8; 16384];
    let mut offset = 0;
    while offset < length {
        let count = (length - offset).min(buffer.len() as u64) as usize;
        // Reads use the locked SQLite file; opening/closing a second source fd would drop POSIX locks.
        let result = unsafe { read(file, buffer.as_mut_ptr().cast(), count as i32, offset as i64) };
        if result != ffi::SQLITE_OK { return Err(Code::StorageUnavailable); }
        output.write_all(&buffer[..count]).map_err(|_| Code::StorageUnavailable)?;
        offset += count as u64;
    }
    Ok(())
}

fn copy_journal(source: &Path, before: &fs::Metadata, destination: &Path) -> Result<(), Code> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut input = options.open(source).map_err(|_| Code::StorageUnavailable)?;
    if !same_file(before, &input.metadata().map_err(|_| Code::StorageUnavailable)?) { return Err(Code::StorageUnavailable); }
    let mut header = [0_u8; 28];
    input.read_exact(&mut header).map_err(|_| Code::Schema)?;
    let original_pages = u32::from_be_bytes(header[16..20].try_into().map_err(|_| Code::Schema)?);
    let sector_bytes = u32::from_be_bytes(header[20..24].try_into().map_err(|_| Code::Schema)?);
    let page_bytes = u32::from_be_bytes(header[24..28].try_into().map_err(|_| Code::Schema)?);
    if header[..8] != JOURNAL_MAGIC || original_pages == 0 || original_pages > 16384
        || page_bytes != 4096 || !(512..=65536).contains(&sector_bytes) || !sector_bytes.is_power_of_two()
        || before.len() < u64::from(sector_bytes) {
        return Err(Code::Schema);
    }
    // Native diagnostics never ATTACHes databases. A super-journal footer could reference other paths.
    input.seek(SeekFrom::End(-8)).map_err(|_| Code::Schema)?;
    let mut magic = [0_u8; 8];
    input.read_exact(&mut magic).map_err(|_| Code::Schema)?;
    if magic == JOURNAL_MAGIC { return Err(Code::Schema); }
    validate_journal_records(&mut input, before.len(), header, original_pages, sector_bytes)?;
    input.seek(SeekFrom::Start(0)).map_err(|_| Code::StorageUnavailable)?;
    let mut output = private_file(destination)?;
    let copied = std::io::copy(&mut Read::by_ref(&mut input).take(MAX_JOURNAL_BYTES + 1), &mut output).map_err(|_| Code::StorageUnavailable)?;
    if copied != before.len() || !same_file(before, &input.metadata().map_err(|_| Code::StorageUnavailable)?) { return Err(Code::StorageUnavailable); }
    Ok(())
}

fn validate_journal_records(input: &mut fs::File, length: u64, mut header: [u8; 28], original_pages: u32, sector_bytes: u32) -> Result<(), Code> {
    let sector = u64::from(sector_bytes);
    let mut offset = sector;
    let mut record = [0_u8; 4104];
    loop {
        let declared = u32::from_be_bytes(header[8..12].try_into().map_err(|_| Code::Schema)?);
        let nonce = u32::from_be_bytes(header[12..16].try_into().map_err(|_| Code::Schema)?);
        let remaining = length.checked_sub(offset).ok_or(Code::Schema)?;
        let count = if declared == u32::MAX {
            // SAFE_APPEND journals have one header and consume the entire remaining file.
            if remaining % record.len() as u64 != 0 { return Err(Code::Schema); }
            remaining / record.len() as u64
        } else { u64::from(declared) };
        if count == 0 || count > remaining / record.len() as u64 { return Err(Code::Schema); }
        input.seek(SeekFrom::Start(offset)).map_err(|_| Code::StorageUnavailable)?;
        for _ in 0..count {
            input.read_exact(&mut record).map_err(|_| Code::Schema)?;
            let page = u32::from_be_bytes(record[..4].try_into().map_err(|_| Code::Schema)?);
            let expected = u32::from_be_bytes(record[4100..].try_into().map_err(|_| Code::Schema)?);
            // Match SQLite's pager_cksum sampling; schema/quick_check cannot detect skipped undo pages.
            let checksum = (96..4096).step_by(200).fold(nonce, |sum, index| sum.wrapping_add(u32::from(record[4 + index])));
            if page == 0 || page > original_pages || checksum != expected { return Err(Code::Schema); }
            offset += record.len() as u64;
        }
        if offset == length { return Ok(()); }
        let next_header = offset.div_ceil(sector) * sector;
        while offset < next_header.min(length) {
            let count = (next_header.min(length) - offset).min(record.len() as u64) as usize;
            input.read_exact(&mut record[..count]).map_err(|_| Code::Schema)?;
            if record[..count].iter().any(|byte| *byte != 0) { return Err(Code::Schema); }
            offset += count as u64;
        }
        if offset == length { return Ok(()); }
        if length - offset < sector { return Err(Code::Schema); }
        input.read_exact(&mut header).map_err(|_| Code::Schema)?;
        // A new, unsynced header has no magic/count; SQLite cannot have spilled its pages yet.
        if header[..12] == [0_u8; 12] { return Ok(()); }
        if header[..8] != JOURNAL_MAGIC
            || u32::from_be_bytes(header[16..20].try_into().map_err(|_| Code::Schema)?) != original_pages
            || u32::from_be_bytes(header[20..24].try_into().map_err(|_| Code::Schema)?) != sector_bytes
            || u32::from_be_bytes(header[24..28].try_into().map_err(|_| Code::Schema)?) != 4096 {
            return Err(Code::Schema);
        }
        offset += sector;
    }
}

fn private_file(path: &Path) -> Result<fs::File, Code> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| Code::StorageUnavailable)
}

fn recovery_metadata(path: &Path, maximum: u64) -> Result<fs::Metadata, Code> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Code::StorageUnavailable)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum { return Err(Code::StorageUnavailable); }
    require_owned_private(&metadata)?;
    Ok(metadata)
}

fn require_owned_private(metadata: &fs::Metadata) -> Result<(), Code> {
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0
            || (metadata.is_file() && metadata.nlink() != 1) {
            return Err(Code::StorageUnavailable);
        }
    }
    #[cfg(not(unix))]
    let _ = metadata;
    Ok(())
}

fn require_unchanged(path: &Path, before: &fs::Metadata) -> Result<(), Code> {
    let after = fs::symlink_metadata(path).map_err(|_| Code::StorageUnavailable)?;
    if !same_file(before, &after) { return Err(Code::StorageUnavailable); }
    Ok(())
}

fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if !after.is_file() || before.len() != after.len() || before.modified().ok() != after.modified().ok() { return false; }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        return before.dev() == after.dev() && before.ino() == after.ino()
            && before.uid() == after.uid() && before.mode() == after.mode() && before.nlink() == after.nlink()
            && before.ctime() == after.ctime() && before.ctime_nsec() == after.ctime_nsec();
    }
    #[cfg(not(unix))]
    { before.created().ok() == after.created().ok() && before.permissions() == after.permissions() }
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    sidecar.into()
}

fn reject_sidecars(path: &Path) -> Result<(), Code> {
    for suffix in ["-journal", "-wal", "-shm"] {
        match fs::symlink_metadata(sidecar_path(path, suffix)) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => return Err(Code::StorageUnavailable),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(Code::StorageUnavailable),
        }
    }
    Ok(())
}

pub(super) fn validate(connection: &Connection) -> Result<(), Code> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|_| Code::Schema)?;
    if version != VERSION { return Err(Code::Schema); }
    // GLOB keeps the underscore literal; LIKE would hide user-defined sqliteX triggers/indexes.
    let mut statement = connection.prepare("SELECT type,name,substr(sql,1,2048),length(sql) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name LIMIT 8").map_err(|_| Code::Schema)?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))).map_err(|_| Code::Schema)?;
    let mut count = 0;
    for row in rows {
        let (kind, name, sql, sql_length) = row.map_err(|_| Code::Schema)?;
        if sql_length > 2048 { return Err(Code::Schema); }
        let expected = if kind == "table" && name == "events" { Some(TABLE) }
            else if kind == "index" { INDEXES.iter().find(|(index, _)| *index == name).map(|(_, sql)| *sql) }
            else { None };
        if expected != Some(sql.as_str()) { return Err(Code::Schema); }
        count += 1;
    }
    if count != INDEXES.len() + 1 { return Err(Code::Schema); }
    let page_size: u32 = connection.pragma_query_value(None, "page_size", |row| row.get(0)).map_err(|_| Code::Schema)?;
    if page_size != 4096 { return Err(Code::Schema); }
    Ok(())
}

/// Reports only schema text already checked against the closed native schema.
pub(super) fn report(connection: &Connection) -> Result<SchemaReport, Code> {
    validate(connection)?;
    let sql = connection.query_row("SELECT sql FROM sqlite_schema WHERE name='events'", [], |row| row.get(0)).map_err(|_| Code::Schema)?;
    let mut statement = connection.prepare("PRAGMA table_info(events)").map_err(|_| Code::Schema)?;
    let columns = statement.query_map([], |row| Ok(SchemaColumn {
        name: row.get(1)?, data_type: row.get(2)?, not_null: row.get::<_, i32>(3)? != 0,
        primary_key: row.get::<_, i32>(5)? != 0,
    })).map_err(|_| Code::Schema)?.collect::<Result<Vec<_>, _>>().map_err(|_| Code::Schema)?;
    let mut statement = connection.prepare("SELECT name,sql FROM sqlite_schema WHERE type='index' AND name IN (?1,?2,?3,?4,?5,?6) ORDER BY name").map_err(|_| Code::Schema)?;
    let indexes = statement.query_map(params_from_iter(INDEXES.iter().map(|(name, _)| *name)), |row| Ok(SchemaIndex { name: row.get(0)?, sql: row.get(1)? }))
        .map_err(|_| Code::Schema)?.collect::<Result<Vec<_>, _>>().map_err(|_| Code::Schema)?;
    Ok(SchemaReport { version: VERSION, sql, columns, indexes })
}

pub(super) fn reject_symlinks(path: &Path) -> Result<(), Code> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        if component == Component::ParentDir { return Err(Code::StorageUnavailable); }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(Code::StorageUnavailable),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(Code::StorageUnavailable),
        }
    }
    Ok(())
}

pub(super) fn prepare_path(path: &Path) -> Result<(), Code> {
    reject_symlinks(path)?;
    let directory = path.parent().filter(|directory| !directory.as_os_str().is_empty()).ok_or(Code::StorageUnavailable)?;
    create_directories(directory)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|_| Code::StorageUnavailable)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => {},
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err(Code::StorageUnavailable),
    }
    reject_symlinks(path)?;
    reject_sidecars(path)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| Code::StorageUnavailable)?;
    }
    Ok(())
}

fn create_directories(path: &Path) -> Result<(), Code> {
    if path.is_dir() { return Ok(()); }
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) { create_directories(parent)?; }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(_) => Err(Code::StorageUnavailable),
    }
}
