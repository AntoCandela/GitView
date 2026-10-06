//! Captures bounded effective remote and ref mapping evidence using read-only Git commands.
use super::*;
use crate::{
    git::process::{ProbeDeadline, ProcessFailure},
    github::model::PrCode,
    workspace::NativeIdentity,
};

const MAX_EVIDENCE: usize = 64 * 1024;
const MAX_REMOTES: usize = 16;
impl AssociationResolver {
    pub(super) async fn capture_local(
        &self,
        context: &SelectedContext,
        branch: Option<&str>,
    ) -> Result<LocalCapture, Failure> {
        if NativeIdentity::capture(&context.root, &context.git_dir)
            .ok()
            .as_ref()
            != Some(&context.identity)
        {
            return Err(PrCode::StaleContext.failure());
        }
        let deadline = ProbeDeadline::new();
        let common = self
            .git_text(
                context,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                deadline,
                false,
            )
            .await?;
        let common_storage = PathBuf::from(common.trim_end())
            .canonicalize()
            .map_err(|_| PrCode::StaleContext.failure())?;
        let config_args = [
            "config",
            "--null",
            "--get-regexp",
            "^(remote\\.|branch\\.|push\\.|url\\.)",
        ];
        let config = self.git_text(context, &config_args, deadline, true).await?;
        let branch_label = match branch {
            Some(branch) => {
                validate_ref(branch)?;
                Some(branch.to_owned())
            }
            None => {
                let symbolic = self
                    .git_text(
                        context,
                        &["symbolic-ref", "--quiet", "HEAD"],
                        deadline,
                        true,
                    )
                    .await?;
                symbolic
                    .trim_end()
                    .strip_prefix("refs/heads/")
                    .map(str::to_owned)
            }
        };
        let mut evidence = Vec::new();
        append(&mut evidence, common.as_bytes())?;
        append(&mut evidence, config.as_bytes())?;
        append(
            &mut evidence,
            branch_label.as_deref().unwrap_or("").as_bytes(),
        )?;
        let mut binding = evidence.clone();
        let mut fields = Vec::new();
        if let Some(branch) = &branch_label {
            let reference = format!("refs/heads/{branch}");
            let refs = self.git_text(context, &["for-each-ref", "--count=2", "--format=%(refname)%00%(upstream:remotename)%00%(upstream:remoteref)%00%(push:remotename)%00%(push:remoteref)%00%(objectname)", &reference], deadline, false).await?;
            // Exact matching matters because for-each-ref also accepts prefix patterns.
            if let Some(line) = refs
                .lines()
                .find(|line| line.split('\0').next() == Some(&reference))
            {
                fields = line.split('\0').map(str::to_owned).collect();
                if fields.len() != 6 {
                    return Err(PrCode::InvalidOutput.failure());
                }
            }
            append(&mut evidence, refs.as_bytes())?;
            append(
                &mut binding,
                fields.get(..5).unwrap_or(&[]).join("\0").as_bytes(),
            )?;
        }
        let names = self.git_text(context, &["remote"], deadline, false).await?;
        if names.lines().count() > MAX_REMOTES {
            return Err(PrCode::ResourceLimit.failure());
        }
        let mut repositories = Vec::new();
        let mut heads = Vec::new();
        let mut unresolved = false;
        let push = fields.get(3).filter(|s| !s.is_empty());
        let selected_remote = push.or_else(|| fields.get(1).filter(|s| !s.is_empty()));
        let selected_ref = remote_head(&fields, &config, branch_label.as_deref());
        for name in names.lines() {
            if name.is_empty() || name.starts_with('-') || name.len() > 1024 {
                return Err(PrCode::InvalidOutput.failure());
            }
            let fetch = self
                .git_text(
                    context,
                    &["remote", "get-url", "--all", name],
                    deadline,
                    false,
                )
                .await?;
            let push_urls = self
                .git_text(
                    context,
                    &["remote", "get-url", "--push", "--all", name],
                    deadline,
                    false,
                )
                .await?;
            append(&mut evidence, name.as_bytes())?;
            append(&mut evidence, fetch.as_bytes())?;
            append(&mut evidence, push_urls.as_bytes())?;
            append(&mut binding, name.as_bytes())?;
            append(&mut binding, fetch.as_bytes())?;
            append(&mut binding, push_urls.as_bytes())?;
            for url in fetch.lines().chain(push_urls.lines()) {
                match parse_url(url) {
                    Some(repo) => {
                        if !repositories.contains(&repo) {
                            repositories.push(repo);
                        }
                    }
                    None => unresolved = true,
                }
            }
            if selected_remote.map(String::as_str) == Some(name) {
                let urls = if push.is_some() { &push_urls } else { &fetch };
                for url in urls.lines() {
                    if let (Some(repository), Some(head_ref)) = (
                        parse_url(url),
                        selected_ref
                            .as_deref()
                            .and_then(|r| r.strip_prefix("refs/heads/")),
                    ) {
                        validate_ref(head_ref)?;
                        let head = LocalHead {
                            repository,
                            head_ref: head_ref.into(),
                        };
                        if !heads.contains(&head) {
                            heads.push(head);
                        }
                    } else {
                        unresolved = true;
                    }
                }
            }
        }
        if repositories.len() > 32 || heads.len() > 16 {
            return Err(PrCode::ResourceLimit.failure());
        }
        if heads.is_empty() {
            unresolved = true;
        }
        // Re-read config after effective URLs so a concurrent edit cannot bless a mixed capture.
        if self.git_text(context, &config_args, deadline, true).await? != config {
            return Err(PrCode::StaleContext.failure());
        }
        if NativeIdentity::capture(&context.root, &context.git_dir)
            .ok()
            .as_ref()
            != Some(&context.identity)
        {
            return Err(PrCode::StaleContext.failure());
        }
        let config_fingerprint = self.fingerprint(&binding);
        Ok(LocalCapture {
            common_storage,
            config_fingerprint,
            branch_label,
            heads,
            repositories,
            unresolved,
            requested_branch: branch.map(str::to_owned),
            evidence,
        })
    }
    async fn git_text(
        &self,
        context: &SelectedContext,
        args: &[&str],
        deadline: ProbeDeadline,
        absent_ok: bool,
    ) -> Result<String, Failure> {
        let output = self
            .git
            .run(Some(&context.root), args, deadline)
            .await
            .map_err(|error| match error.failure {
                ProcessFailure::Deadline => PrCode::Timeout.failure(),
                ProcessFailure::OutputLimit => PrCode::ResourceLimit.failure(),
                _ => PrCode::RepositoryUnavailable.failure(),
            })?;
        if !output.status.success() && !(absent_ok && output.status.code() == Some(1)) {
            return Err(PrCode::UnresolvedMapping.failure());
        }
        String::from_utf8(output.stdout).map_err(|_| PrCode::UnresolvedMapping.failure())
    }
    fn fingerprint(&self, evidence: &[u8]) -> String {
        let mut entries = self.fingerprints.lock();
        if let Some(index) = entries.iter().position(|(value, _)| value == evidence) {
            let entry = entries.remove(index);
            let id = entry.1.clone();
            entries.push(entry);
            return id;
        }
        while entries.len() >= 64
            || entries
                .iter()
                .map(|(v, id)| v.capacity() + id.capacity())
                .sum::<usize>()
                + evidence.len()
                + 36
                + 64 * std::mem::size_of::<(Vec<u8>, String)>()
                > 2 * 1024 * 1024
        {
            entries.remove(0);
        }
        let id = uuid::Uuid::new_v4().to_string();
        entries.push((evidence.to_vec(), id.clone()));
        id
    }
}
fn append(evidence: &mut Vec<u8>, value: &[u8]) -> Result<(), Failure> {
    if evidence.len() + value.len() + 8 > MAX_EVIDENCE {
        return Err(PrCode::ResourceLimit.failure());
    }
    evidence.extend_from_slice(&(value.len() as u64).to_le_bytes());
    evidence.extend_from_slice(value);
    Ok(())
}
pub(super) fn validate_ref(value: &str) -> Result<(), Failure> {
    if value.is_empty()
        || value.len() > 512
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || value
            .split('/')
            .any(|s| s.starts_with('.') || s.ends_with(".lock"))
        || value
            .bytes()
            .any(|b| b <= 32 || b == 127 || b"~^:?*[\\".contains(&b))
    {
        return Err(PrCode::UnresolvedMapping.failure());
    }
    Ok(())
}
pub(super) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
pub(super) fn parse_url(raw: &str) -> Option<RepositoryName> {
    let path = if let Some(path) = raw.strip_prefix("git@github.com:") {
        path
    } else {
        let url = url::Url::parse(raw).ok()?;
        if url.host_str()? != "github.com"
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.password().is_some()
        {
            return None;
        }
        match url.scheme() {
            "https" if url.username().is_empty() => {}
            "ssh" if url.username() == "git" => {}
            _ => return None,
        }
        return parse_path(url.path().strip_prefix('/')?);
    };
    parse_path(path)
}
fn parse_path(path: &str) -> Option<RepositoryName> {
    let (owner, name) = path.split_once('/')?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    if !valid_name(owner) || !valid_name(name) {
        return None;
    }
    Some(RepositoryName {
        owner: owner.into(),
        name: name.into(),
    })
}

// Git can report push:remotename but leave push:remoteref empty for push.default.
// Fill only the documented deterministic modes; matching/unknown mappings stay unresolved.
fn remote_head(fields: &[String], config: &str, branch: Option<&str>) -> Option<String> {
    let upstream = fields.get(2)?;
    let push_remote = fields.get(3)?;
    let push_ref = fields.get(4)?;
    if push_remote.is_empty() {
        return (!upstream.is_empty()).then(|| upstream.clone());
    }
    if !push_ref.is_empty() {
        return Some(push_ref.clone());
    }
    let get = |key: &str| {
        config
            .split('\0')
            .filter_map(|entry| entry.split_once('\n'))
            .filter(|(name, _)| *name == key)
            .map(|(_, value)| value)
            .last()
    };
    if get(&format!("remote.{push_remote}.push")).is_some() {
        return None;
    }
    let branch = branch?;
    let local_ref = format!("refs/heads/{branch}");
    match get("push.default").unwrap_or("simple") {
        "current" => Some(local_ref),
        "upstream" | "tracking" if fields.get(1) == Some(push_remote) && !upstream.is_empty() => {
            Some(upstream.clone())
        }
        "simple" if fields.get(1) != Some(push_remote) || *upstream == local_ref => Some(local_ref),
        _ => None,
    }
}
