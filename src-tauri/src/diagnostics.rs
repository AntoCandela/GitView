//! Captures fixed native metadata in private versioned SQLite without delaying producers.
//!
//! Host startup owns the writer; a separate explicit-path CLI owns read-only retrieval.
//! No repository payload, raw native error, renderer SQL, or upload crosses this boundary.

mod reader;
mod schema;
mod store;
mod types;

pub use reader::{Query, QueryResult, ReadOnlyDiagnostics, StoredEvent};
pub use schema::{SchemaColumn, SchemaIndex, SchemaReport};
pub use store::{DiagnosticSink, DiagnosticStore};
pub use types::{Code, Component, DiagnosticDetails, DiagnosticHealth, DiagnosticRecord, Event, HealthState, Level, OperationContext, OperationKind};
