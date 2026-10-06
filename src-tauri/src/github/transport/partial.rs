//! Keeps optional field failures local without accepting partial identity or authorization facts.
use super::{Connection, GhRead};
use serde_json::Value;

pub(super) fn project(body: &mut Value, read: Option<&GhRead>) -> bool {
    let Some(errors) = body.get("errors").and_then(Value::as_array) else {
        return false;
    };
    if errors.is_empty() || errors.len() > 100 {
        return false;
    }
    let mut fields = Vec::new();
    for error in errors {
        // Any access/rate/unknown code fails the whole read, even alongside a field-local error.
        if [
            error.get("type"),
            error.pointer("/extensions/type"),
            error.pointer("/extensions/code"),
        ]
        .into_iter()
        .flatten()
        .any(|kind| {
            !matches!(
                kind.as_str(),
                Some("INTERNAL" | "INTERNAL_SERVER_ERROR" | "SERVICE_UNAVAILABLE")
            )
        }) {
            return false;
        }
        let Some(path) = error.get("path").and_then(Value::as_array) else {
            return false;
        };
        let field = match read {
            Some(GhRead::ReadOverview { .. })
                if path == &["repository", "pullRequest", "body"].map(Value::from) =>
            {
                "/data/repository/pullRequest/body".to_owned()
            }
            Some(GhRead::ReadConnection {
                connection: Connection::Threads,
                ..
            }) if (6..=16).contains(&path.len())
                && path[..4]
                    == ["repository", "pullRequest", "reviewThreads", "nodes"].map(Value::from)
                && path[5] == "comments" =>
            {
                let Some(index) = path[4].as_u64().filter(|index| *index < 100) else {
                    return false;
                };
                format!("/data/repository/pullRequest/reviewThreads/nodes/{index}/comments")
            }
            _ => return false,
        };
        if body.pointer(&field).is_none() {
            return false;
        }
        fields.push(field);
    }
    // Validate every error before discarding any field or its error evidence.
    for field in fields {
        *body.pointer_mut(&field).expect("validated optional field") = Value::Null;
    }
    body.as_object_mut()
        .expect("object containing errors")
        .remove("errors");
    true
}
