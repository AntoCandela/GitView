//! Reads only the explicitly maintainer-authorized diagnostic store; never creates it.

use gitview_lib::diagnostics::{Code, Component, Event, Level, Query, ReadOnlyDiagnostics};
use std::{ffi::OsString, io::{self, Write}, path::PathBuf, str::FromStr};
use uuid::Uuid;

fn main() {
    if let Err(code) = run(std::env::args_os().skip(1)) {
        eprintln!("{{\"error\":\"{}\"}}", code.as_str());
        std::process::exit(1);
    }
}

fn run(mut arguments: impl Iterator<Item = OsString>) -> Result<(), Code> {
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--database")) { return Err(Code::InvalidArguments); }
    let path = PathBuf::from(arguments.next().ok_or(Code::InvalidArguments)?);
    if path.as_os_str().is_empty() { return Err(Code::InvalidArguments); }
    let command = arguments.next().ok_or(Code::InvalidArguments)?;
    let command = command.to_str().ok_or(Code::InvalidArguments)?;
    match command {
        "schema" => {
            if arguments.next().is_some() { return Err(Code::InvalidArguments); }
            write_json(&ReadOnlyDiagnostics::open(path)?.schema()?)
        },
        "events" => {
            let query = filters(arguments)?;
            write_json(&ReadOnlyDiagnostics::open(path)?.events(&query)?)
        },
        _ => Err(Code::InvalidArguments),
    }
}

fn filters(mut arguments: impl Iterator<Item = OsString>) -> Result<Query, Code> {
    let mut query = Query::default();
    let mut seen = 0u8;
    while let Some(option) = arguments.next() {
        let option = option.to_str().ok_or(Code::InvalidArguments)?;
        let bit = match option {
            "--level" => 1, "--component" => 2, "--event" => 4, "--operation-id" => 8,
            "--session-id" => 16, "--since-ms" => 32, "--limit" => 64,
            _ => return Err(Code::InvalidArguments),
        };
        if seen & bit != 0 { return Err(Code::InvalidArguments); }
        seen |= bit;
        let value = arguments.next().ok_or(Code::InvalidArguments)?;
        let value = value.to_str().ok_or(Code::InvalidArguments)?;
        if value.len() > 64 { return Err(Code::InvalidArguments); }
        match option {
            "--level" => query.level = Some(Level::from_str(value)?),
            "--component" => query.component = Some(Component::from_str(value)?),
            "--event" => query.event = Some(Event::from_str(value)?),
            "--operation-id" => query.operation_id = Some(parse_id(value)?),
            "--session-id" => query.session_id = Some(parse_id(value)?),
            "--since-ms" => {
                let timestamp = value.parse().map_err(|_| Code::InvalidArguments)?;
                if timestamp < 0 { return Err(Code::InvalidArguments); }
                query.since_ms = Some(timestamp);
            },
            "--limit" => {
                query.limit = value.parse().map_err(|_| Code::InvalidArguments)?;
                if !(1..=200).contains(&query.limit) { return Err(Code::InvalidArguments); }
            },
            _ => unreachable!(),
        }
    }
    Ok(query)
}

fn parse_id(value: &str) -> Result<Uuid, Code> {
    let id = Uuid::parse_str(value).map_err(|_| Code::InvalidArguments)?;
    let mut canonical = Uuid::encode_buffer();
    if id.get_version() != Some(uuid::Version::Random) || id.get_variant() != uuid::Variant::RFC4122 || id.hyphenated().encode_lower(&mut canonical) != value {
        return Err(Code::InvalidArguments);
    }
    Ok(id)
}

fn write_json(value: &impl serde::Serialize) -> Result<(), Code> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, value).map_err(|_| Code::Storage)?;
    stdout.write_all(b"\n").map_err(|_| Code::Storage)
}
