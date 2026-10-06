//! Verifies independent connection authority and coherent provider observation versions.
use super::*;
use serde_json::json;
fn identity() -> PrIdentity {
    PrIdentity {
        host: "github.com".into(),
        base_repository_id: "1".into(),
        number: 7,
    }
}
fn version() -> PrVersion {
    PrVersion {
        base_oid: Some("a".repeat(40)),
        head_oid: Some("b".repeat(40)),
        lifecycle: Lifecycle::Open,
        updated_at: "2025-01-01T00:00:00Z".into(),
    }
}
fn page(kind: CollectionKind) -> PageAuthority {
    PageAuthority {
        identity: identity(),
        version: version(),
        collection: kind,
        thread_provider_id: None,
        thread_id: None,
    }
}
fn envelope(connection: &str, nodes: Value) -> Value {
    let mut pull = json!({"number":7,"baseRefOid":"a".repeat(40),"headRefOid":"b".repeat(40),"state":"OPEN","updatedAt":"2025-01-01T00:00:00Z"});
    pull[connection] =
        json!({"nodes":nodes,"totalCount":0,"pageInfo":{"hasNextPage":false,"endCursor":null}});
    json!({"data":{"repository":{"databaseId":1,"pullRequest":pull}}})
}
#[test]
fn github_discussion_timeline_cursor_cannot_authorize_nested_comment_page() {
    let cursor = CursorAuthority {
        page: page(CollectionKind::Timeline),
        cursor: "timeline-next".into(),
        provider_order: 1,
        seen_ids: vec!["event-one".into()],
        pages: 1,
        items: 1,
        provider_limited: false,
    };
    let mut request = page(CollectionKind::ThreadComments);
    request.thread_provider_id = Some("provider-thread".into());
    request.thread_id = Some("native-thread".into());
    assert_eq!(
        normalize_page(&json!({}), &request, Some(&cursor), 1)
            .err()
            .unwrap()
            .code,
        PrCode::StaleCursor
    );
}
#[test]
fn github_discussion_force_push_rejects_mixed_commit_membership() {
    let mut body = envelope("commits", json!([]));
    body["data"]["repository"]["pullRequest"]["headRefOid"] = json!("c".repeat(40));
    assert_eq!(
        normalize_page(&body, &page(CollectionKind::Commits), None, 1)
            .err()
            .unwrap()
            .code,
        PrCode::ChangedSnapshot
    );
}

fn overview_body(body: Value) -> Value {
    let mut result = envelope("labels", json!([]));
    let repo = &mut result["data"]["repository"];
    repo["owner"] = json!({"login":"fixture"});
    repo["name"] = json!("project");
    let pull = &mut repo["pullRequest"];
    pull["title"] = json!("Readable title");
    pull["body"] = body;
    pull["baseRefName"] = json!("main");
    pull["headRefName"] = Value::Null;
    pull["headRepository"] = Value::Null;
    pull["author"] = Value::Null;
    pull["createdAt"] = json!("2024-12-01T00:00:00Z");
    pull["isDraft"] = json!(false);
    result
}
#[test]
fn github_discussion_overview_preserves_prose_source_states_and_nullable_metadata() {
    let source =
        "# Review\n<script>alert('inert')</script>\n![alt](https://external.example/image)";
    let result = normalize_overview(&overview_body(json!(source)), &identity(), 3).unwrap();
    assert!(matches!(result.overview.body,Prose::Available{text} if text==source));
    assert!(result.overview.head_repository.is_none());
    assert!(result.overview.head_ref.is_none());
    assert!(result.overview.author.is_none());
    assert!(result.overview.counts.files.is_none());
    assert!(result.overview.review_decision.is_none());
    assert!(matches!(
        result.overview.labels.completeness,
        Completeness::Limited
    ));
    assert_eq!(result.overview.labels.observed_revision, 3);
    assert!(matches!(
        normalize_overview(&overview_body(json!("")), &identity(), 1)
            .unwrap()
            .overview
            .body,
        Prose::Empty
    ));
    assert!(matches!(
        normalize_overview(&overview_body(Value::Null), &identity(), 1)
            .unwrap()
            .overview
            .body,
        Prose::Unavailable { .. }
    ));
    let long = "€".repeat(MAX_PROSE);
    let result = normalize_overview(&overview_body(json!(long)), &identity(), 1).unwrap();
    assert!(
        matches!(result.overview.body,Prose::Limited{text} if text.len()<=MAX_PROSE && text.chars().all(|c|c=='€'))
    );
}
#[test]
fn github_discussion_wrong_identity_and_partial_connection_errors_never_become_empty_success() {
    let mut body = overview_body(json!("description"));
    body["data"]["repository"]["databaseId"] = json!(2);
    assert_eq!(
        normalize_overview(&body, &identity(), 1)
            .err()
            .unwrap()
            .code,
        PrCode::InvalidOutput
    );
    let mut body = envelope("timelineItems", json!([]));
    body["errors"] = json!([{"path":["repository","pullRequest","timelineItems"],"type":"OTHER"}]);
    assert_eq!(
        normalize_page(&body, &page(CollectionKind::Timeline), None, 1)
            .err()
            .unwrap()
            .code,
        PrCode::InvalidOutput
    );
}
fn timeline_node(kind: &str) -> Value {
    json!({"__typename":kind,"id":kind,"createdAt":"2025-01-01T00:00:00Z","actor":null})
}
#[test]
fn github_discussion_curated_timeline_retains_all_categories_and_commit_time_meaning() {
    let mut nodes = Vec::new();
    for kind in [
        "ConvertToDraftEvent",
        "ReadyForReviewEvent",
        "HeadRefForcePushedEvent",
        "BaseRefForcePushedEvent",
        "BaseRefChangedEvent",
        "ReviewRequestedEvent",
        "ReviewRequestRemovedEvent",
        "ReviewDismissedEvent",
        "ClosedEvent",
        "ReopenedEvent",
        "MergedEvent",
    ] {
        let mut node = timeline_node(kind);
        node["beforeCommit"] = json!({"oid":"a".repeat(40)});
        node["afterCommit"] = json!({"oid":"b".repeat(40)});
        node["previousRefName"] = json!("main");
        node["currentRefName"] = json!("release");
        node["review"] = json!({"id":"review-one"});
        node["previousReviewState"] = json!("APPROVED");
        node["dismissalMessage"] = json!("Please review again");
        nodes.push(node);
    }
    nodes.push(json!({"__typename":"IssueComment","id":"comment-one","body":"Readable comment","createdAt":"2025-01-01T00:00:00Z","author":null}));
    nodes.push(json!({"__typename":"PullRequestReview","id":"review-one","body":"","state":"APPROVED","submittedAt":"2025-01-01T00:00:00Z","author":null}));
    nodes.push(json!({"__typename":"PullRequestCommit","id":"commit-event","commit":{"oid":"c".repeat(40),"authoredDate":"2000-01-01T00:00:00Z","committedDate":"2001-01-01T00:00:00Z"}}));
    nodes.push(nodes[11].clone());
    let mut body = envelope("timelineItems", json!(nodes));
    let pull = &mut body["data"]["repository"]["pullRequest"];
    pull["createdAt"] = json!("2024-01-01T00:00:00Z");
    let output = normalize_page(&body, &page(CollectionKind::Timeline), None, 1).unwrap();
    let events: Vec<_> = output
        .collection
        .items
        .iter()
        .map(|item| match item {
            Item::Timeline { event } => event,
            _ => panic!("wrong kind"),
        })
        .collect();
    assert_eq!(events.len(), 15);
    assert!(matches!(events[0].kind, TimelineKind::Opened));
    assert!(matches!(
        output.collection.completeness,
        Completeness::Complete
    ));
    let kinds: Vec<_> = events
        .iter()
        .map(|event| serde_json::to_value(event.kind).unwrap())
        .collect();
    assert_eq!(
        json!(kinds),
        json!([
            "opened",
            "draft_changed",
            "draft_changed",
            "force_pushed",
            "force_pushed",
            "base_changed",
            "review_requested",
            "review_removed",
            "review_dismissed",
            "closed",
            "reopened",
            "merged",
            "comment",
            "review_submitted",
            "commit"
        ])
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, TimelineKind::Comment))
            .count(),
        1
    );
    let review = events
        .iter()
        .find(|event| matches!(event.kind, TimelineKind::ReviewSubmitted))
        .unwrap();
    assert!(
        matches!(&review.details,TimelineDetails::Review{body:Prose::Empty,state,..} if state=="APPROVED")
    );
    let commit = events.last().unwrap();
    assert!(matches!(commit.kind, TimelineKind::Commit));
    assert!(commit.occurred_at.is_none());
    assert!(
        matches!(&commit.details,TimelineDetails::Commit{authored_at:Some(at),..} if at=="2000-01-01T00:00:00Z")
    );
    assert!(events
        .windows(2)
        .all(|pair| pair[0].provider_order < pair[1].provider_order));
    assert!(events.iter().any(|event| matches!(
        &event.details,
        TimelineDetails::ForcePushed { is_base: true, .. }
    )));
    assert!(events.iter().any(|event|matches!(&event.details,TimelineDetails::Comment{comment} if matches!(&comment.body,Prose::Available{text} if text=="Readable comment"))));
}
fn cursor_from(output: &NormalizedPage) -> CursorAuthority {
    output
        .grants
        .iter()
        .find_map(|grant| match &grant.resource {
            crate::github::service::Resource::Cursor {
                target: crate::github::service::CursorTarget::Discussion(authority),
                ..
            } => Some((**authority).clone()),
            _ => None,
        })
        .unwrap()
}
#[test]
fn github_discussion_unknown_coverage_and_duplicates_remain_limited_across_pages() {
    let mut body = envelope(
        "timelineItems",
        json!([{"__typename":"FutureEvent","id":"future"}]),
    );
    body["data"]["repository"]["pullRequest"]["timelineItems"]["pageInfo"] =
        json!({"hasNextPage":true,"endCursor":"next"});
    let first = normalize_page(&body, &page(CollectionKind::Timeline), None, 1).unwrap();
    assert!(matches!(
        first.collection.completeness,
        Completeness::Limited
    ));
    let cursor = cursor_from(&first);
    assert!(cursor.provider_limited);
    let second = envelope(
        "timelineItems",
        json!([{"__typename":"FutureEvent","id":"future"},timeline_node("ReopenedEvent")]),
    );
    let output =
        normalize_page(&second, &page(CollectionKind::Timeline), Some(&cursor), 1).unwrap();
    assert_eq!(output.collection.items.len(), 1);
    assert!(matches!(
        output.collection.completeness,
        Completeness::Limited
    ));
    assert!(output.collection.next_cursor.is_none());
}
#[test]
fn github_discussion_user_team_reviewers_and_deleted_request_targets_stay_distinct() {
    let body = envelope(
        "reviewRequests",
        json!([
            {"id":"r1","requestedReviewer":{"__typename":"User","id":"u1","login":"fixture","name":null}},
            {"id":"r2","requestedReviewer":{"__typename":"Team","id":"t1","name":"Review team","slug":"review-team"}},
            {"id":"r3","requestedReviewer":null}
        ]),
    );
    let output = normalize_page(&body, &page(CollectionKind::Reviewers), None, 1).unwrap();
    assert!(matches!(
        output.collection.items[0],
        Item::Reviewer {
            reviewer: Reviewer::User {
                requested: true,
                ..
            }
        }
    ));
    assert!(matches!(
        output.collection.items[1],
        Item::Reviewer {
            reviewer: Reviewer::Team { .. }
        }
    ));
    assert!(matches!(
        output.collection.completeness,
        Completeness::Limited
    ));
}
fn root_comment() -> Value {
    json!({"id":"root","body":"Please keep context","createdAt":"2025-01-01T00:00:00Z","author":null,"replyTo":null,"path":"src/lib.rs","diffHunk":"@@ -1 +1 @@\n-old\n+new","url":"https://github.com/fixture/project/pull/7#discussion_r1","originalCommit":{"oid":"c".repeat(40)},"commit":{"oid":"b".repeat(40)},"pullRequestReview":{"id":"review-one"}})
}
fn thread_node() -> Value {
    json!({"id":"provider-thread","path":"src/lib.rs","line":8,"startLine":null,"originalLine":3,"originalStartLine":null,"diffSide":"RIGHT","isResolved":false,"isOutdated":true,"comments":{"totalCount":2,"pageInfo":{"hasNextPage":true,"endCursor":"comments-next"},"nodes":[root_comment()]}})
}
#[test]
fn github_discussion_threads_keep_original_anchor_and_independent_reply_cursor() {
    let body = envelope("reviewThreads", json!([thread_node()]));
    let output = normalize_page(&body, &page(CollectionKind::Threads), None, 1).unwrap();
    let Item::Thread { thread } = &output.collection.items[0] else {
        panic!("thread")
    };
    assert_eq!(thread.review_id.as_deref(), Some("review-one"));
    assert_eq!(thread.comments.items.len(), 1);
    let cursor = cursor_from(&output);
    assert_eq!(cursor.page.collection, CollectionKind::ThreadComments);
    assert_eq!(cursor.page.thread_id.as_deref(), Some(thread.id.as_str()));
    let anchor = output
        .grants
        .iter()
        .find_map(|g| match &g.resource {
            crate::github::service::Resource::Anchor { authority } => Some(authority),
            _ => None,
        })
        .unwrap();
    assert!(anchor.current.is_none());
    assert_eq!(anchor.original.as_ref().unwrap().line, 3);
    assert_eq!(anchor.original.as_ref().unwrap().commit_oid, "c".repeat(40));
    let response = json!({"data":{"node":{"id":"provider-thread","pullRequest":{"number":7,"repository":{"databaseId":1},"baseRefOid":"a".repeat(40),"headRefOid":"b".repeat(40),"state":"OPEN","updatedAt":"2025-01-02T00:00:00Z"},"comments":{"totalCount":2,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"id":"reply","body":"Reply body","replyTo":{"id":"root"}}]}}}});
    let replies = normalize_page(&response, &cursor.page, Some(&cursor), 1).unwrap();
    assert!(
        matches!(&replies.collection.items[0],Item::Comment{comment} if comment.reply_to_id.as_deref()==Some("root") && matches!(&comment.body,Prose::Available{text} if text=="Reply body"))
    );
    let mut wrong = cursor.page.clone();
    wrong.thread_provider_id = Some("different-thread".into());
    assert_eq!(
        normalize_page(&response, &wrong, Some(&cursor), 1)
            .err()
            .unwrap()
            .code,
        PrCode::StaleCursor
    );
}
#[test]
fn github_discussion_missing_or_unmappable_thread_context_keeps_excerpt_without_anchor() {
    let mut node = thread_node();
    node["path"] = json!("renamed.rs");
    let output = normalize_page(
        &envelope("reviewThreads", json!([node])),
        &page(CollectionKind::Threads),
        None,
        1,
    )
    .unwrap();
    assert!(
        matches!(&output.collection.items[0],Item::Thread{thread} if thread.anchor_id.is_none() && matches!(&thread.diff_excerpt,Prose::Available{text} if text.contains("-old")))
    );
    let mut node = thread_node();
    node["comments"] = Value::Null;
    let output = normalize_page(
        &envelope("reviewThreads", json!([node])),
        &page(CollectionKind::Threads),
        None,
        1,
    )
    .unwrap();
    assert!(
        matches!(&output.collection.items[0],Item::Thread{thread} if thread.anchor_id.is_none() && matches!(thread.comments.completeness,Completeness::Limited))
    );
}
#[test]
fn github_discussion_twentieth_page_retains_data_but_cannot_issue_unbounded_continuation() {
    let mut cursor = CursorAuthority {
        page: page(CollectionKind::Labels),
        cursor: "nineteen".into(),
        provider_order: 19,
        seen_ids: vec![],
        pages: 19,
        items: 19,
        provider_limited: false,
    };
    let mut body = envelope(
        "labels",
        json!([{"id":"label","name":"Ready","color":"ffffff"}]),
    );
    body["data"]["repository"]["pullRequest"]["labels"]["pageInfo"] =
        json!({"hasNextPage":true,"endCursor":"twenty"});
    let output = normalize_page(&body, &cursor.page, Some(&cursor), 1).unwrap();
    assert_eq!(output.collection.items.len(), 1);
    assert!(matches!(
        output.collection.limit_reason,
        Some(LimitReason::Resource)
    ));
    assert!(output.collection.next_cursor.is_none());
    cursor.cursor = "twenty".into();
    assert_eq!(
        normalize_page(&body, &cursor.page, Some(&cursor), 1)
            .err()
            .unwrap()
            .code,
        PrCode::InvalidOutput
    );
}

#[test]
fn github_discussion_commit_grants_preserve_ordered_parents_and_explicit_parent_limits() {
    let commit = json!({"id":"membership","commit":{"oid":"d".repeat(40),"message":"Merge title\nDetails","authoredDate":"2000-01-01T00:00:00Z","committedDate":"2001-01-01T00:00:00Z","parents":{"totalCount":3,"pageInfo":{"hasNextPage":true},"nodes":[{"oid":"b".repeat(40)},{"oid":"a".repeat(40)}]}}});
    let output = normalize_page(
        &envelope("commits", json!([commit])),
        &page(CollectionKind::Commits),
        None,
        1,
    )
    .unwrap();
    let authority = output
        .grants
        .iter()
        .find_map(|grant| match &grant.resource {
            crate::github::service::Resource::Commit {
                authority,
                parent_count,
            } => Some((authority, parent_count)),
            _ => None,
        })
        .unwrap();
    assert_eq!(*authority.1, 3);
    assert!(!authority.0.parents_complete);
    assert_eq!(authority.0.parents, vec!["b".repeat(40), "a".repeat(40)]);
    assert!(
        matches!(&output.collection.items[0],Item::Commit{title,authored_at:Some(at),..} if title=="Merge title" && at=="2000-01-01T00:00:00Z")
    );
}

#[test]
fn github_discussion_pending_review_is_not_a_submitted_review_event() {
    let body = envelope(
        "timelineItems",
        json!([{"__typename":"PullRequestReview","id":"pending","state":"PENDING","body":"Unsubmitted text","submittedAt":null,"author":null}]),
    );
    let output = normalize_page(&body, &page(CollectionKind::Timeline), None, 1).unwrap();
    assert!(
        matches!(&output.collection.items[0],Item::Timeline{event} if matches!(event.kind,TimelineKind::Unsupported) && event.occurred_at.is_none())
    );
    assert!(matches!(
        output.collection.completeness,
        Completeness::Limited
    ));
}
