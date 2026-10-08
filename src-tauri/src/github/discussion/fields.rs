//! Parses bounded provider scalars while retaining prose as inert, unmodified text.
use super::*;

pub(super) fn text(value: &Value, max: usize) -> Result<String, Failure> {
    value
        .as_str()
        .filter(|s| bounded(s, max))
        .map(str::to_owned)
        .ok_or_else(|| PrCode::InvalidOutput.failure())
}
pub(super) fn optional(value: &Value, max: usize) -> Result<Option<String>, Failure> {
    if value.is_null() {
        Ok(None)
    } else {
        text(value, max).map(Some)
    }
}
pub(super) fn optional_oid(value: &Value) -> Result<Option<String>, Failure> {
    let value = optional(value, 64)?;
    if value.as_deref().is_some_and(|s| !oid(s)) {
        return Err(PrCode::InvalidOutput.failure());
    }
    Ok(value)
}
pub(super) fn count(value: &Value) -> Result<Option<u64>, Failure> {
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_u64()
            .map(Some)
            .ok_or_else(|| PrCode::InvalidOutput.failure())
    }
}
pub(super) fn timestamp(value: &Value) -> Result<Option<String>, Failure> {
    let value = optional(value, 64)?;
    if value.as_ref().is_some_and(|s| {
        s.len() < 20
            || s.as_bytes().get(4) != Some(&b'-')
            || s.as_bytes().get(7) != Some(&b'-')
            || s.as_bytes().get(10) != Some(&b'T')
            || !s.ends_with('Z')
    }) {
        return Err(PrCode::InvalidOutput.failure());
    }
    Ok(value)
}
pub(super) fn prose(value: &Value) -> Prose {
    match value.as_str() {
        Some("") => Prose::Empty,
        Some(value) if value.len() <= MAX_PROSE => Prose::Available { text: value.into() },
        Some(value) => {
            let mut end = MAX_PROSE;
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            Prose::Limited {
                text: value[..end].into(),
            }
        }
        None => Prose::Unavailable {
            code: PrCode::InvalidOutput,
        },
    }
}
pub(super) fn actor(value: &Value) -> Result<Option<Person>, Failure> {
    if value.is_null() {
        return Ok(None);
    }
    if !value.is_object() {
        return Err(PrCode::InvalidOutput.failure());
    }
    Ok(Some(Person {
        provider_id: optional(&value["id"], MAX_ID)?,
        login: optional(&value["login"], 100)?,
        display_name: optional(&value["name"], 512)?,
    }))
}
pub(super) fn reviewer(value: &Value) -> Result<Option<Reviewer>, Failure> {
    if value.is_null() {
        return Ok(None);
    }
    match value["__typename"].as_str() {
        Some("User") => Ok(Some(Reviewer::User {
            actor: actor(value)?.ok_or_else(|| PrCode::InvalidOutput.failure())?,
            requested: true,
            submitted: None,
        })),
        Some("Team") => Ok(Some(Reviewer::Team {
            provider_id: text(&value["id"], MAX_ID)?,
            name: text(&value["name"], 512)?,
            slug: text(&value["slug"], 100)?,
            requested: true,
        })),
        _ => Ok(None),
    }
}
pub(super) fn repository(value: &Value) -> Result<GithubRepository, Failure> {
    let id = repository_id(value)?;
    let owner = text(&value["owner"]["login"], 100)?;
    let name = text(&value["name"], 100)?;
    if [&owner, &name].iter().any(|s| {
        !s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || matches!(s.as_str(), "." | "..")
    }) {
        return Err(PrCode::InvalidOutput.failure());
    }
    Ok(GithubRepository {
        id,
        host: GithubHost::GithubCom,
        url: format!("https://github.com/{owner}/{name}"),
        owner,
        name,
    })
}
pub(super) fn repository_id(value: &Value) -> Result<String, Failure> {
    value["databaseId"]
        .as_u64()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or_else(|| PrCode::InvalidOutput.failure())
}
pub(super) fn version(value: &Value) -> Result<PrVersion, Failure> {
    if value.get("baseRefOid").is_none() || value.get("headRefOid").is_none() {
        return Err(PrCode::InvalidOutput.failure());
    }
    let lifecycle = match value["state"].as_str() {
        Some("OPEN") => Lifecycle::Open,
        Some("CLOSED") => Lifecycle::Closed,
        Some("MERGED") => Lifecycle::Merged,
        _ => return Err(PrCode::InvalidOutput.failure()),
    };
    Ok(PrVersion {
        base_oid: optional_oid(&value["baseRefOid"])?,
        head_oid: optional_oid(&value["headRefOid"])?,
        lifecycle,
        updated_at: timestamp(&value["updatedAt"])?
            .ok_or_else(|| PrCode::InvalidOutput.failure())?,
    })
}
pub(super) fn envelope<'a>(
    body: &'a Value,
    identity: &PrIdentity,
    thread: Option<&str>,
) -> Result<(&'a Value, &'a Value), Failure> {
    if !valid_identity(identity)
        || body
            .get("errors")
            .is_some_and(|errors| !errors.as_array().is_some_and(Vec::is_empty))
    {
        return Err(PrCode::InvalidOutput.failure());
    }
    let (repository, pull) = if let Some(thread) = thread {
        let node = &body["data"]["node"];
        if node["id"].as_str() != Some(thread) {
            return Err(PrCode::StaleCursor.failure());
        }
        (&node["pullRequest"]["repository"], &node["pullRequest"])
    } else {
        (
            &body["data"]["repository"],
            &body["data"]["repository"]["pullRequest"],
        )
    };
    if repository_id(repository)? != identity.base_repository_id
        || pull["number"].as_u64() != Some(identity.number)
    {
        return Err(PrCode::InvalidOutput.failure());
    }
    Ok((repository, pull))
}
pub(super) fn limited<T>(revision: u64) -> Collection<T> {
    Collection {
        items: vec![],
        total_count: None,
        next_cursor: None,
        completeness: Completeness::Limited,
        limit_reason: Some(LimitReason::Provider),
        observed_revision: revision,
    }
}
pub(super) fn canonical_link(value: &Value, identity: &PrIdentity) -> Option<String> {
    let raw = value.as_str()?;
    if !bounded(raw, 4096) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
    {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.collect();
    if segments.len() != 4
        || segments[2] != "pull"
        || segments[3] != identity.number.to_string()
        || segments[..2].iter().any(|s| {
            s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return None;
    }
    Some(url.to_string())
}
pub(super) fn link(
    value: &Value,
    identity: &PrIdentity,
    grants: &mut Vec<Grant>,
) -> Option<String> {
    let url = canonical_link(value, identity)?;
    let grant = Grant::new(super::super::service::Resource::Link { url });
    let id = grant.id().to_owned();
    grants.push(grant);
    Some(id)
}
