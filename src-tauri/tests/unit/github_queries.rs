//! Query-boundary fixtures: provider data stays in variables and paging retains identity evidence.
use super::{build, Connection, GhRead};
use serde_json::Value;

fn document(connection: Connection, cursor: Option<String>) -> Value {
    let input = build(&GhRead::ReadConnection {
        owner: "fixture-owner".into(), repository: "fixture-repo".into(), number: 42,
        connection, cursor,
    }).expect("closed query builds");
    assert_eq!(input.args[0..2], ["api", "graphql"]);
    serde_json::from_slice(&input.input.unwrap()).unwrap()
}

#[test]
fn github_queries_provider_values_are_only_json_variables() {
    let cursor = "cursor\"}){viewer{login}} #";
    let body = document(Connection::Timeline, Some(cursor.into()));
    let query = body["query"].as_str().unwrap();
    assert!(!query.contains(cursor));
    assert!(!query.contains("fixture-owner"));
    assert_eq!(body["variables"]["cursor"], cursor);
    assert_eq!(body["variables"]["count"], 100);
    assert_eq!(body["variables"]["number"], 42);
}

#[test]
fn github_queries_all_pages_carry_identity_revision_and_lifecycle() {
    for connection in [Connection::Commits, Connection::Timeline, Connection::Threads,
        Connection::Reviewers, Connection::Labels, Connection::ThreadComments { thread_id: "thread-node".into() }] {
        let body = document(connection, None);
        let query = body["query"].as_str().unwrap();
        for field in ["databaseId", "number", "headRefOid", "baseRefOid", "updatedAt", "state", "mergedAt", "isDraft", "totalCount", "pageInfo{hasNextPage endCursor}"] {
            assert!(query.contains(field), "missing {field}");
        }
    }
}

#[test]
fn github_queries_thread_paging_and_original_context_are_independent() {
    let first = document(Connection::Threads, Some("thread-cursor".into()));
    let query = first["query"].as_str().unwrap();
    assert!(query.contains("reviewThreads(first:$count,after:$cursor)"));
    assert!(query.contains("comments(first:20){totalCount pageInfo"));
    for field in ["originalCommit{oid}", "commit{oid}", "replyTo{id}", "diffHunk", "originalLine", "originalStartLine", "startDiffSide", "subjectType"] {
        assert!(query.contains(field), "missing anchor evidence {field}");
    }
    let next = document(Connection::ThreadComments { thread_id: "thread-node".into() }, Some("comment-cursor".into()));
    assert_eq!(next["variables"]["id"], "thread-node");
    assert_eq!(next["variables"]["cursor"], "comment-cursor");
    let query = next["query"].as_str().unwrap();
    assert!(query.contains("pullRequest{"));
    assert!(query.contains("repository{databaseId"));
    assert!(query.contains("comments(first:$count,after:$cursor)"));
    assert!(!query.contains("thread-node"));
}

#[test]
fn github_queries_timeline_covers_curated_provider_categories() {
    let body = document(Connection::Timeline, None);
    let query = body["query"].as_str().unwrap();
    for category in ["IssueComment", "PullRequestReview", "PullRequestCommit", "ConvertToDraftEvent", "ReadyForReviewEvent", "HeadRefForcePushedEvent", "BaseRefForcePushedEvent", "BaseRefChangedEvent", "ReviewRequestedEvent", "ReviewRequestRemovedEvent", "ReviewDismissedEvent", "ClosedEvent", "ReopenedEvent", "MergedEvent"] {
        assert!(query.contains(&format!("... on {category}{{")), "missing {category}");
    }
    assert!(query.contains("__typename"));
    assert!(query.contains("authoredDate committedDate"));
    assert!(query.contains("parents(first:100){totalCount pageInfo{hasNextPage endCursor} nodes{oid}}"));
    assert!(query.contains("createdAt url author"));
}

#[test]
fn github_queries_overview_and_reviewer_metadata_are_complete() {
    let input = build(&GhRead::ReadOverview { owner: "owner".into(), repository: "repo".into(), number: 1 }).unwrap();
    let body: Value = serde_json::from_slice(&input.input.unwrap()).unwrap();
    let query = body["query"].as_str().unwrap();
    for field in ["title body", "headRefName", "baseRefName", "headRepository", "reviewDecision", "changedFiles", "additions", "deletions", "commits{totalCount}", "createdAt", "closedAt", "mergedAt"] {
        assert!(query.contains(field), "missing overview {field}");
    }
    assert!(!query.contains("reviewRequests("));
    let reviewers = document(Connection::Reviewers, None);
    let query = reviewers["query"].as_str().unwrap();
    assert!(query.contains("... on User{id login name}"));
    assert!(query.contains("... on Team{id name slug organization{login}}"));
}

#[test]
fn github_queries_reject_unbounded_or_invalid_inputs() {
    for number in [0, i32::MAX as u64 + 1] {
        assert!(build(&GhRead::ReadOverview { owner: "owner".into(), repository: "repo".into(), number }).is_err());
    }
    for cursor in ["x".repeat(4097), "bad\ncursor".into()] {
        assert!(build(&GhRead::ReadConnection { owner: "owner".into(), repository: "repo".into(), number: 1, connection: Connection::Timeline, cursor: Some(cursor) }).is_err());
    }
    assert!(build(&GhRead::ReadConnection { owner: "owner".into(), repository: "repo".into(), number: 1, connection: Connection::ThreadComments { thread_id: "x".repeat(1025) }, cursor: None }).is_err());
}
