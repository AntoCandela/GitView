//! Exposes parameterized bounded queries without writes, SQL injection, or raw payloads.

use super::{schema, types::{parse_id, valid_id}, Code, Component, Event, Level, OperationKind, SchemaReport};
use rusqlite::{params_from_iter, types::Value, Connection, Row};
use serde::Serialize;
use std::{path::Path, str::FromStr};
use uuid::Uuid;

#[derive(Debug)]
pub struct Query {
    pub level: Option<Level>,
    pub component: Option<Component>,
    pub event: Option<Event>,
    pub operation_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub since_ms: Option<i64>,
    pub limit: u16,
}
impl Default for Query {
    fn default() -> Self { Self { level: None, component: None, event: None, operation_id: None, session_id: None, since_ms: None, limit: 20 } }
}

#[derive(Debug, Serialize)]
pub struct StoredEvent {
    pub id: i64,
    pub timestamp_ms: i64,
    pub session_id: Uuid,
    pub operation_id: Uuid,
    pub parent_operation_id: Option<Uuid>,
    pub operation_kind: OperationKind,
    pub level: Level,
    pub component: Component,
    pub event: Event,
    pub code: Option<Code>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub stdout_bytes: Option<u64>,
    pub stderr_bytes: Option<u64>,
    pub cleanup_failed: bool,
}

#[derive(Debug, Serialize)]
pub struct QueryResult { pub events: Vec<StoredEvent>, pub has_more: bool }

pub struct ReadOnlyDiagnostics { connection: Connection }

impl ReadOnlyDiagnostics {
    /// An explicit maintainer-granted path is the CLI's OS-account authorization boundary.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Code> {
        Ok(Self { connection: schema::read_connection(path.as_ref())? })
    }
    pub fn schema(&self) -> Result<SchemaReport, Code> { schema::report(&self.connection) }

    pub fn events(&self, query: &Query) -> Result<QueryResult, Code> {
        if !(1..=200).contains(&query.limit) || query.since_ms.is_some_and(|value| value < 0)
            || query.operation_id.is_some_and(|id| !valid_id(id)) || query.session_id.is_some_and(|id| !valid_id(id)) {
            return Err(Code::InvalidArguments);
        }
        let mut sql = String::from("SELECT id,timestamp_ms,session_id,operation_id,parent_operation_id,operation_kind,level,component,event,code,duration_ms,exit_code,stdout_bytes,stderr_bytes,cleanup_failed FROM events");
        let mut clauses = Vec::with_capacity(6);
        let mut values = Vec::with_capacity(7);
        for (column, value) in [
            ("level", query.level.map(Level::as_str)),
            ("component", query.component.map(Component::as_str)),
            ("event", query.event.map(Event::as_str)),
        ] {
            if let Some(value) = value { clauses.push(format!("{column}=?")); values.push(Value::Text(value.into())); }
        }
        for (column, value) in [("operation_id", query.operation_id), ("session_id", query.session_id)] {
            if let Some(value) = value { clauses.push(format!("{column}=?")); values.push(Value::Text(value.hyphenated().to_string())); }
        }
        if let Some(since) = query.since_ms { clauses.push("timestamp_ms>=?".into()); values.push(Value::Integer(since)); }
        if !clauses.is_empty() { sql.push_str(" WHERE "); sql.push_str(&clauses.join(" AND ")); }
        sql.push_str(" ORDER BY id DESC LIMIT ?");
        values.push(Value::Integer(i64::from(query.limit) + 1));
        let mut statement = self.connection.prepare(&sql).map_err(|_| Code::Storage)?;
        let mut rows = statement.query(params_from_iter(values)).map_err(|_| Code::Storage)?;
        let mut events = Vec::with_capacity(usize::from(query.limit) + 1);
        while let Some(row) = rows.next().map_err(|_| Code::Storage)? { events.push(read_event(row)?); }
        let has_more = events.len() > usize::from(query.limit);
        if has_more { events.pop(); }
        Ok(QueryResult { events, has_more })
    }
}

fn read_event(row: &Row<'_>) -> Result<StoredEvent, Code> {
    let id: i64 = get(row, 0)?;
    let timestamp_ms: i64 = get(row, 1)?;
    let cleanup_failed: i64 = get(row, 14)?;
    if id < 1 || timestamp_ms < 0 || !matches!(cleanup_failed, 0 | 1) { return Err(Code::InvalidRecord); }
    Ok(StoredEvent {
        id, timestamp_ms, session_id: read_id(row, 2)?, operation_id: read_id(row, 3)?,
        parent_operation_id: read_text(row, 4)?.map(|id| parse_id(id).map_err(|_| Code::InvalidRecord)).transpose()?,
        operation_kind: read_enum(row, 5)?, level: read_enum(row, 6)?, component: read_enum(row, 7)?, event: read_enum(row, 8)?,
        code: read_text(row, 9)?.map(|code| Code::from_str(code).map_err(|_| Code::InvalidRecord)).transpose()?,
        duration_ms: read_unsigned(row, 10)?, exit_code: get(row, 11)?,
        stdout_bytes: read_unsigned(row, 12)?, stderr_bytes: read_unsigned(row, 13)?, cleanup_failed: cleanup_failed == 1,
    })
}

fn get<T: rusqlite::types::FromSql>(row: &Row<'_>, index: usize) -> Result<T, Code> {
    row.get(index).map_err(|_| Code::InvalidRecord)
}
fn read_id(row: &Row<'_>, index: usize) -> Result<Uuid, Code> {
    parse_id(read_text(row, index)?.ok_or(Code::InvalidRecord)?).map_err(|_| Code::InvalidRecord)
}
fn read_enum<T: FromStr>(row: &Row<'_>, index: usize) -> Result<T, Code> {
    read_text(row, index)?.ok_or(Code::InvalidRecord)?.parse().map_err(|_| Code::InvalidRecord)
}
fn read_unsigned(row: &Row<'_>, index: usize) -> Result<Option<u64>, Code> {
    get::<Option<i64>>(row, index)?.map(|value| u64::try_from(value).map_err(|_| Code::InvalidRecord)).transpose()
}

fn read_text<'a>(row: &'a Row<'_>, index: usize) -> Result<Option<&'a str>, Code> {
    match row.get_ref(index).map_err(|_| Code::InvalidRecord)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Text(bytes) if bytes.len() <= 64 => {
            Ok(Some(std::str::from_utf8(bytes).map_err(|_| Code::InvalidRecord)?))
        },
        _ => Err(Code::InvalidRecord),
    }
}
