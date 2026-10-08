//! Bundled GitHub GraphQL selections. Only these fixed documents enter the transport.
//! Field contracts: https://docs.github.com/en/graphql/reference/pulls
use super::Connection;

const REPOSITORY: &str = "databaseId name nameWithOwner url owner{login}";
const VERSION: &str = "number headRefOid baseRefOid updatedAt state isDraft mergedAt closedAt createdAt url author{... on Node{id} login ... on User{name}}";
const PAGE: &str = "totalCount pageInfo{hasNextPage endCursor}";
const COMMIT: &str = "oid message authoredDate committedDate url parents(first:100){totalCount pageInfo{hasNextPage endCursor} nodes{oid}}";
const REVIEWER: &str = "__typename ... on User{id login name} ... on Team{id name slug organization{login}}";
const COMMENT: &str = "id body createdAt updatedAt url author{... on Node{id} login ... on User{name}} replyTo{id} originalCommit{oid} commit{oid} diffHunk path line startLine originalLine originalStartLine outdated pullRequestReview{id}";
const THREAD: &str = "id path line startLine originalLine originalStartLine diffSide startDiffSide isResolved isOutdated subjectType";

pub(super) fn overview() -> String {
    let fields = format!("title body headRefName baseRefName reviewDecision changedFiles additions deletions commits{{totalCount}} repository{{{REPOSITORY}}} headRepository{{{REPOSITORY}}}");
    format!("query GitViewOverview($owner:String!,$repo:String!,$number:Int!){{repository(owner:$owner,name:$repo){{{REPOSITORY} pullRequest(number:$number){{{VERSION} {fields}}}}}}}")
}

pub(super) fn connection(connection: &Connection) -> String {
    let field = match connection {
        Connection::Commits => format!("commits(first:$count,after:$cursor){{{PAGE} nodes{{id commit{{{COMMIT}}}}}}}"),
        Connection::Timeline => format!("timelineItems(first:$count,after:$cursor){{{PAGE} nodes{{{}}}}}", timeline()),
        // The nested first page has its own pageInfo and never consumes the thread cursor.
        Connection::Threads => format!("reviewThreads(first:$count,after:$cursor){{{PAGE} nodes{{{THREAD} comments(first:20){{{PAGE} nodes{{{COMMENT}}}}}}}}}"),
        Connection::Reviewers => format!("reviewRequests(first:$count,after:$cursor){{{PAGE} nodes{{id requestedReviewer{{{REVIEWER}}}}}}}"),
        Connection::Labels => format!("labels(first:$count,after:$cursor){{{PAGE} nodes{{id name color}}}}"),
        Connection::ThreadComments { .. } => return format!("query GitViewThread($id:ID!,$cursor:String,$count:Int!){{node(id:$id){{... on PullRequestReviewThread{{{THREAD} pullRequest{{{VERSION} repository{{{REPOSITORY}}}}} comments(first:$count,after:$cursor){{{PAGE} nodes{{{COMMENT}}}}}}}}}}}"),
    };
    format!("query GitViewConnection($owner:String!,$repo:String!,$number:Int!,$cursor:String,$count:Int!){{repository(owner:$owner,name:$repo){{{REPOSITORY} pullRequest(number:$number){{{VERSION} {field}}}}}}}")
}

fn timeline() -> String {
    let actor = "id createdAt actor{... on Node{id} login ... on User{name}}";
    format!("__typename
        ... on IssueComment{{id body createdAt updatedAt url author{{... on Node{{id}} login ... on User{{name}}}}}}
        ... on PullRequestReview{{id body submittedAt state url author{{... on Node{{id}} login ... on User{{name}}}}}}
        ... on PullRequestCommit{{id commit{{{COMMIT}}}}}
        ... on ConvertToDraftEvent{{{actor}}}
        ... on ReadyForReviewEvent{{{actor}}}
        ... on HeadRefForcePushedEvent{{{actor} beforeCommit{{oid}} afterCommit{{oid}}}}
        ... on BaseRefForcePushedEvent{{{actor} beforeCommit{{oid}} afterCommit{{oid}}}}
        ... on BaseRefChangedEvent{{{actor} previousRefName currentRefName}}
        ... on ReviewRequestedEvent{{{actor} requestedReviewer{{{REVIEWER}}}}}
        ... on ReviewRequestRemovedEvent{{{actor} requestedReviewer{{{REVIEWER}}}}}
        ... on ReviewDismissedEvent{{{actor} dismissalMessage previousReviewState review{{id}} url}}
        ... on ClosedEvent{{{actor} url}}
        ... on ReopenedEvent{{{actor}}}
        ... on MergedEvent{{{actor} commit{{oid}} url}}")
}
