//! Owns a bounded nonblocking producer and a single durable SQLite writer.

use super::{schema, Code, DiagnosticHealth, DiagnosticRecord, HealthState};
use rusqlite::{params, Connection, OpenFlags};
use std::{path::Path, sync::{atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}, Arc, Mutex, MutexGuard}, thread::{self, JoinHandle}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use uuid::Uuid;

const QUEUE_CAPACITY: usize = 1024;
const MAX_EVENTS: i64 = 20000;
const MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

struct Health {
    state: AtomicU8,
    accepted: AtomicU64,
    written: AtomicU64,
    dropped: AtomicU64,
    last_error: AtomicU8,
    notice_reported: AtomicBool,
    storage_failed: AtomicBool,
}

impl Health {
    fn new(state: HealthState) -> Self {
        Self { state: AtomicU8::new(state as u8), accepted: AtomicU64::new(0), written: AtomicU64::new(0), dropped: AtomicU64::new(0), last_error: AtomicU8::new(u8::MAX), notice_reported: AtomicBool::new(false), storage_failed: AtomicBool::new(false) }
    }
    fn snapshot(&self) -> DiagnosticHealth {
        let state = match self.state.load(Ordering::Acquire) { 0 => HealthState::Healthy, 1 => HealthState::Degraded, 2 => HealthState::Disabled, _ => HealthState::Stopped };
        DiagnosticHealth { state, accepted: self.accepted.load(Ordering::Acquire), written: self.written.load(Ordering::Acquire), dropped: self.dropped.load(Ordering::Acquire), last_error_code: Code::ALL.get(usize::from(self.last_error.load(Ordering::Acquire))).copied() }
    }
    fn degrade(&self, code: Code) {
        self.last_error.store(code as u8, Ordering::Release);
        self.state.store(HealthState::Degraded as u8, Ordering::Release);
    }
    fn notify(&self) {
        if self.state.load(Ordering::Acquire) == HealthState::Degraded as u8 && !self.notice_reported.swap(true, Ordering::AcqRel) {
            if let Some(code) = self.snapshot().last_error_code {
                eprintln!("gitview diagnostics degraded: {}", code.as_str());
            }
        }
    }
    fn drop_record(&self, code: Code) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
        self.degrade(code);
    }
}

struct Admission { sender: SyncSender<Message>, closed: bool }
struct Shared { admission: Mutex<Admission>, health: Health, stop: AtomicBool }
enum Message { Record { record: DiagnosticRecord, timestamp_ms: i64 }, Flush(SyncSender<Result<(), Code>>), Shutdown(SyncSender<Result<(), Code>>) }

/// Clones do no database I/O; contention or a full queue drops the record visibly.
#[derive(Clone)]
pub struct DiagnosticSink { shared: Arc<Shared> }

impl Default for DiagnosticSink {
    fn default() -> Self {
        let (sender, _) = mpsc::sync_channel(QUEUE_CAPACITY);
        Self { shared: Arc::new(Shared { admission: Mutex::new(Admission { sender, closed: true }), health: Health::new(HealthState::Disabled), stop: AtomicBool::new(true) }) }
    }
}

impl DiagnosticSink {
    pub fn health(&self) -> DiagnosticHealth { self.shared.health.snapshot() }

    pub fn record(&self, record: DiagnosticRecord) -> bool {
        if self.shared.stop.load(Ordering::Acquire) {
            self.shared.health.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        if !record.valid() { self.shared.health.drop_record(Code::InvalidRecord); return false; }
        let Ok(admission) = self.shared.admission.try_lock() else {
            self.shared.health.drop_record(Code::Overflow); return false;
        };
        if admission.closed || self.shared.stop.load(Ordering::Acquire) { self.shared.health.dropped.fetch_add(1, Ordering::Relaxed); return false; }
        match admission.sender.try_send(Message::Record { record, timestamp_ms: timestamp_ms() }) {
            Ok(()) => { self.shared.health.accepted.fetch_add(1, Ordering::Relaxed); true },
            Err(TrySendError::Full(_)) => { self.shared.health.drop_record(Code::Overflow); false },
            Err(TrySendError::Disconnected(_)) => { self.shared.health.drop_record(Code::Storage); false },
        }
    }
}

/// Startup owns initialization; explicit paths must be host-resolved or isolated test paths.
pub struct DiagnosticStore { sink: DiagnosticSink, writer: Mutex<Option<JoinHandle<()>>> }

impl DiagnosticStore {
    /// Reports failed host path resolution without attempting a guessed filesystem location.
    pub fn unavailable(code: Code) -> Self {
        let sink = DiagnosticSink::default();
        sink.shared.health.storage_failed.store(true, Ordering::Release);
        sink.shared.health.degrade(code);
        sink.shared.health.notify();
        Self { sink, writer: Mutex::new(None) }
    }

    /// Failure is capture degradation, never an application startup failure.
    pub fn open(path: impl AsRef<Path>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Arc::new(Shared { admission: Mutex::new(Admission { sender, closed: false }), health: Health::new(HealthState::Healthy), stop: AtomicBool::new(false) });
        let result = initialize(path.as_ref()).and_then(|(connection, stored_count)| {
            let shared = shared.clone();
            thread::Builder::new().name("gitview-diagnostics".into()).spawn(move || writer_loop(connection, stored_count, receiver, shared)).map_err(|_| Code::StorageUnavailable)
        });
        let writer = match result {
            Ok(writer) => Some(writer),
            Err(code) => {
                shared.health.storage_failed.store(true, Ordering::Release);
                shared.health.degrade(code);
                shared.health.notify();
                shared.admission.lock().unwrap_or_else(|poison| poison.into_inner()).closed = true;
                None
            },
        };
        Self { sink: DiagnosticSink { shared }, writer: Mutex::new(writer) }
    }

    pub fn sink(&self) -> DiagnosticSink { self.sink.clone() }
    pub fn health(&self) -> DiagnosticHealth { self.sink.health() }

    /// Acknowledges every earlier accepted record after its FULL-synchronous commit.
    pub fn flush(&self, timeout: Duration) -> Result<(), Code> {
        self.control(timeout, false)
    }

    /// Seals all producer clones before draining; timeout never claims persistence.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), Code> {
        let started = Instant::now();
        let result = self.control(timeout, true);
        let deadline = started.checked_add(timeout).ok_or(Code::InvalidArguments)?;
        let mut writer = lock_until(&self.writer, deadline).inspect_err(|code| self.sink.shared.health.degrade(*code))?;
        if let Some(handle) = writer.as_ref() {
            while !handle.is_finished() && Instant::now() < deadline { thread::sleep(Duration::from_millis(1)); }
            if !handle.is_finished() { self.sink.shared.health.degrade(Code::Timeout); return Err(Code::Timeout); }
        }
        if let Some(handle) = writer.take() { if handle.join().is_err() { self.sink.shared.health.degrade(Code::Storage); return Err(Code::Storage); } }
        result
    }

    fn control(&self, timeout: Duration, shutdown: bool) -> Result<(), Code> {
        let deadline = Instant::now().checked_add(timeout).ok_or(Code::InvalidArguments)?;
        let mut admission = match lock_until(&self.sink.shared.admission, deadline) {
            Ok(admission) => admission,
            Err(code) => {
                if shutdown { self.sink.shared.stop.store(true, Ordering::Release); }
                self.sink.shared.health.degrade(code);
                return Err(code);
            },
        };
        if admission.closed {
            return if self.sink.shared.health.storage_failed.load(Ordering::Acquire) { Err(Code::Storage) }
                else if shutdown { Ok(()) } else { Err(Code::Shutdown) };
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        if shutdown { admission.closed = true; }
        let mut message = if shutdown { Message::Shutdown(sender) } else { Message::Flush(sender) };
        loop {
            match admission.sender.try_send(message) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) if Instant::now() < deadline => { message = returned; thread::sleep(Duration::from_millis(1)); },
                Err(TrySendError::Full(_)) => {
                    if shutdown { self.sink.shared.stop.store(true, Ordering::Release); }
                    self.sink.shared.health.degrade(Code::Timeout); return Err(Code::Timeout);
                },
                Err(TrySendError::Disconnected(_)) => { self.sink.shared.health.degrade(Code::Storage); return Err(Code::Storage); },
            }
        }
        drop(admission);
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(result) => result,
            Err(_) => { self.sink.shared.health.degrade(Code::Timeout); Err(Code::Timeout) },
        }
    }
}

impl Drop for DiagnosticStore {
    fn drop(&mut self) { let _ = self.shutdown(Duration::from_millis(500)); }
}

fn lock_until<T>(mutex: &Mutex<T>, deadline: Instant) -> Result<MutexGuard<'_, T>, Code> {
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(poison)) => return Ok(poison.into_inner()),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Err(_) => return Err(Code::Timeout),
        }
    }
}

fn initialize(path: &Path) -> Result<(Connection, i64), Code> {
    schema::reject_symlinks(path)?;
    let recovered = if path.exists() { schema::capture_connection(path)? } else { None };
    schema::prepare_path(path)?;
    let connection = match recovered {
        Some(connection) => connection,
        None => Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW).map_err(|_| Code::StorageUnavailable)?,
    };
    connection.busy_timeout(Duration::from_millis(100)).map_err(|_| Code::Storage)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|_| Code::Schema)?;
    if version == 0 {
        connection.pragma_update(None, "page_size", 4096).map_err(|_| Code::Storage)?;
        let transaction = connection.unchecked_transaction().map_err(|_| Code::Storage)?;
        transaction.execute_batch(schema::TABLE).map_err(|_| Code::Storage)?;
        for (_, sql) in schema::INDEXES { transaction.execute_batch(sql).map_err(|_| Code::Storage)?; }
        transaction.pragma_update(None, "user_version", schema::VERSION).map_err(|_| Code::Storage)?;
        transaction.commit().map_err(|_| Code::Storage)?;
    }
    schema::validate(&connection)?;
    let journal_mode: String = connection.pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0)).map_err(|_| Code::Storage)?;
    if journal_mode != "delete" { return Err(Code::Storage); }
    connection.pragma_update(None, "synchronous", "FULL").map_err(|_| Code::Storage)?;
    let page_count: u32 = connection.pragma_query_value(None, "page_count", |row| row.get(0)).map_err(|_| Code::Storage)?;
    if page_count > 16384 { return Err(Code::Storage); }
    let maximum_pages: u32 = connection.pragma_update_and_check(None, "max_page_count", 16384, |row| row.get(0)).map_err(|_| Code::Storage)?;
    if maximum_pages != 16384 { return Err(Code::Storage); }
    let stored_count = connection.query_row("SELECT count(*) FROM events", [], |row| row.get(0)).map_err(|_| Code::Storage)?;
    Ok((connection, stored_count))
}

fn writer_loop(mut connection: Connection, mut stored_count: i64, receiver: Receiver<Message>, shared: Arc<Shared>) {
    let mut session_buffer = Uuid::encode_buffer();
    let session: &str = Uuid::new_v4().hyphenated().encode_lower(&mut session_buffer);
    loop {
        shared.health.notify();
        let message = match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) if !shared.stop.load(Ordering::Acquire) => continue,
            Err(_) => break,
        };
        match message {
            Message::Record { record, timestamp_ms } => {
                if shared.health.storage_failed.load(Ordering::Acquire) {
                    shared.health.dropped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                match write_record(&mut connection, &session, &mut stored_count, timestamp_ms, record) {
                    Ok(()) => { shared.health.written.fetch_add(1, Ordering::Release); },
                    Err(code) => {
                        shared.health.storage_failed.store(true, Ordering::Release);
                        shared.health.drop_record(code);
                    },
                }
            },
            Message::Flush(sender) => { let _ = sender.send(commit_status(&shared)); },
            Message::Shutdown(sender) => {
                let status = commit_status(&shared);
                shared.health.notify();
                drop(connection);
                if status.is_ok() { let _ = shared.health.state.compare_exchange(HealthState::Healthy as u8, HealthState::Stopped as u8, Ordering::AcqRel, Ordering::Acquire); }
                let _ = sender.send(status);
                return;
            },
        }
    }
}

fn commit_status(shared: &Shared) -> Result<(), Code> {
    if shared.health.storage_failed.load(Ordering::Acquire) { Err(Code::Storage) } else { Ok(()) }
}

fn write_record(connection: &mut Connection, session: &str, stored_count: &mut i64, timestamp: i64, record: DiagnosticRecord) -> Result<(), Code> {
    let transaction = connection.transaction().map_err(|_| Code::Storage)?;
    let expired = transaction.execute("DELETE FROM events WHERE timestamp_ms < ?1", [timestamp.saturating_sub(MAX_AGE_MS)]).map_err(|_| Code::Storage)? as i64;
    let remaining = stored_count.saturating_sub(expired);
    let excess = (remaining + 1 - MAX_EVENTS).max(0);
    if excess > 0 {
        transaction.execute("DELETE FROM events WHERE id IN (SELECT id FROM events ORDER BY id LIMIT ?1)", [excess]).map_err(|_| Code::Storage)?;
    }
    let details = record.details;
    let mut operation_buffer = Uuid::encode_buffer();
    let operation: &str = record.operation_id.hyphenated().encode_lower(&mut operation_buffer);
    let mut parent_buffer = Uuid::encode_buffer();
    let parent: Option<&str> = match record.parent_operation_id.as_ref() {
        Some(id) => Some(id.hyphenated().encode_lower(&mut parent_buffer)),
        None => None,
    };
    transaction.execute("INSERT INTO events(timestamp_ms,session_id,operation_id,parent_operation_id,operation_kind,level,component,event,code,duration_ms,exit_code,stdout_bytes,stderr_bytes,cleanup_failed) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)", params![
        timestamp, session, operation, parent, record.operation_kind.as_str(), record.level.as_str(), record.component.as_str(), record.event.as_str(), record.code.map(Code::as_str), details.duration_ms.map(|v| v as i64), details.exit_code, details.stdout_bytes.map(|v| v as i64), details.stderr_bytes.map(|v| v as i64), details.cleanup_failed,
    ]).map_err(|_| Code::Storage)?;
    transaction.commit().map_err(|_| Code::Storage)?;
    *stored_count = remaining + 1 - excess;
    Ok(())
}

fn timestamp_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis().min(i64::MAX as u128) as i64).unwrap_or(0)
}
