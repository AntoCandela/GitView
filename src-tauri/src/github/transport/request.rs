//! Builds only bundled read operations; provider values never become executable or query text.
use serde_json::json;
use super::{PrCode, PAGE_SIZE};

pub(crate) enum Connection { Commits, Timeline, Threads, ThreadComments { thread_id: String }, Reviewers, Labels }

/// Native-only inputs. Repository routing is literal github.com owner/name, never a URL.
pub(crate) enum GhRead {
    ProbeVersion,
    ProbeAuth,
    ReadViewer,
    ReadRepository { owner: String, repository: String },
    ListPulls { owner: String, repository: String, head: Option<String>, page: u32 },
    ReadPull { owner: String, repository: String, number: u64 },
    ReadConnection { owner: String, repository: String, number: u64, connection: Connection, cursor: Option<String> },
    ReadCommit { owner: String, repository: String, oid: String, page: u32 },
    ReadPullFiles { owner: String, repository: String, number: u64, page: u32 },
}

pub(super) struct CommandInput { pub args: Vec<String>, pub input: Option<Vec<u8>> }

pub(super) fn build(read: &GhRead) -> Result<CommandInput, PrCode> {
    match read {
        GhRead::ProbeVersion => return Ok(CommandInput { args: vec!["version".into()], input: None }),
        GhRead::ProbeAuth => return Ok(CommandInput { args: ["auth", "status", "--active", "--hostname", "github.com", "--json", "hosts"].map(str::to_owned).to_vec(), input: None }),
        GhRead::ReadViewer => return graphql("query GitViewViewer { viewer { id } }", json!({})),
        _ => {}
    }
    let (owner, repository) = match read {
        GhRead::ReadRepository { owner, repository } | GhRead::ListPulls { owner, repository, .. } |
        GhRead::ReadPull { owner, repository, .. } | GhRead::ReadConnection { owner, repository, .. } |
        GhRead::ReadCommit { owner, repository, .. } | GhRead::ReadPullFiles { owner, repository, .. } => (owner, repository),
        _ => unreachable!(),
    };
    validate_name(owner, 100)?;
    validate_name(repository, 100)?;
    let base = format!("repos/{}/{}", encode(owner), encode(repository));
    let endpoint = match read {
        GhRead::ReadRepository { .. } => base,
        GhRead::ListPulls { head, page, .. } => {
            validate_page(*page)?;
            let mut endpoint = format!("{base}/pulls?state=all&per_page={PAGE_SIZE}&page={page}");
            if let Some(head) = head { validate_variable(head, 1024)?; endpoint.push_str(&format!("&head={}", encode(head))); }
            endpoint
        }
        GhRead::ReadPull { number, .. } => { validate_number(*number)?; format!("{base}/pulls/{number}") }
        GhRead::ReadPullFiles { number, page, .. } => {
            validate_number(*number)?; validate_page(*page)?;
            format!("{base}/pulls/{number}/files?per_page={PAGE_SIZE}&page={page}")
        }
        GhRead::ReadCommit { oid, page, .. } => {
            validate_page(*page)?;
            if ![40,64].contains(&oid.len()) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(PrCode::InvalidOutput); }
            format!("{base}/commits/{oid}?per_page={PAGE_SIZE}&page={page}")
        }
        GhRead::ReadConnection { number, connection, cursor, .. } => {
            validate_number(*number)?;
            if let Some(cursor) = cursor { validate_variable(cursor, 4096)?; }
            let mut variables = json!({"owner":owner,"repo":repository,"number":number,"cursor":cursor,"count":PAGE_SIZE});
            let field = match connection {
                Connection::Commits => "commits(first:$count,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{commit{oid message committedDate parents(first:100){nodes{oid}}}}}",
                Connection::Timeline => "timelineItems(first:$count,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{__typename ... on IssueComment{id body createdAt updatedAt url author{login}} ... on PullRequestReview{id body submittedAt state url author{login}}}}",
                Connection::Threads => "reviewThreads(first:$count,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{id path line originalLine diffSide isResolved isOutdated}}",
                Connection::Reviewers => "reviewRequests(first:$count,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{requestedReviewer{__typename ... on User{id login name} ... on Team{id name slug}}}}",
                Connection::Labels => "labels(first:$count,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{id name color}}",
                Connection::ThreadComments { thread_id } => {
                    validate_variable(thread_id, 1024)?;
                    variables = json!({"id":thread_id,"cursor":cursor,"count":PAGE_SIZE});
                    return graphql("query GitViewThread($id:ID!,$cursor:String,$count:Int!){node(id:$id){... on PullRequestReviewThread{id comments(first:$count,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{id body createdAt updatedAt url diffHunk author{login}}}}}}", variables);
                }
            };
            // Only a compile-time selected field is composed; all provider data is JSON variables.
            return graphql(&format!("query GitViewConnection($owner:String!,$repo:String!,$number:Int!,$cursor:String,$count:Int!){{repository(owner:$owner,name:$repo){{pullRequest(number:$number){{{field}}}}}}}"), variables);
        }
        _ => unreachable!(),
    };
    Ok(CommandInput { args: api_args(endpoint, "GET"), input: None })
}

fn api_args(endpoint: String, method: &str) -> Vec<String> {
    vec!["api".into(), endpoint, "--hostname".into(), "github.com".into(), "--method".into(), method.into(),
        "--include".into(), "--header".into(), "Accept: application/vnd.github+json".into(),
        "--header".into(), "X-GitHub-Api-Version: 2022-11-28".into()]
}
fn graphql(query: &str, variables: serde_json::Value) -> Result<CommandInput, PrCode> {
    let mut args = api_args("graphql".into(), "POST");
    args.extend(["--input".into(), "-".into(), "--header".into(), "Content-Type: application/json".into()]);
    let input = serde_json::to_vec(&json!({"query":query,"variables":variables})).map_err(|_| PrCode::InvalidOutput)?;
    Ok(CommandInput { args, input: Some(input) })
}
fn validate_name(value: &str, limit: usize) -> Result<(), PrCode> {
    if value.is_empty() || value.len() > limit || value == "." || value == ".." || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) { return Err(PrCode::InvalidOutput); }
    Ok(())
}
fn validate_variable(value: &str, limit: usize) -> Result<(), PrCode> {
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) { return Err(PrCode::InvalidOutput); } Ok(())
}
fn validate_number(number: u64) -> Result<(), PrCode> { if number == 0 || number > i32::MAX as u64 { Err(PrCode::InvalidOutput) } else { Ok(()) } }
fn validate_page(page: u32) -> Result<(), PrCode> { if page == 0 { Err(PrCode::InvalidOutput) } else { Ok(()) } }
fn encode(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}
