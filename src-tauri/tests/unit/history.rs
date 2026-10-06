//! Covers raw topology preservation and whole-response history authority validation.

use super::*;

fn hash(byte: char) -> String { byte.to_string().repeat(40) }
fn commit(parents: &[&str], message: &[u8]) -> Vec<u8> {
    let mut bytes = format!("tree {}\n", hash('a')).into_bytes();
    for parent in parents { bytes.extend_from_slice(format!("parent {parent}\n").as_bytes()); }
    bytes.extend_from_slice(b"author Fixture <fixture@example.org> 1 +0000\ncommitter Fixture <fixture@example.org> 1 +0000\n\n");
    bytes.extend_from_slice(message);
    bytes
}

#[test]
fn raw_merge_parent_order_is_preserved_even_when_traversal_would_simplify_it() {
    let left = hash('b');
    let right = hash('c');
    let merge = reader::parse_commit(&hash('d'), &commit(&[&right, &left], b"merge subject\nbody\n")).unwrap();
    assert_eq!(merge.parents.iter().map(|parent| parent.oid.as_str()).collect::<Vec<_>>(), [right.as_str(), left.as_str()]);
    assert!(!merge.root);
    assert_eq!(merge.subject.as_deref(), Some("merge subject"));
}

#[test]
fn non_utf8_commit_subject_is_omitted_without_losing_real_parent_topology() {
    let parent = hash('b');
    let row = reader::parse_commit(&hash('d'), &commit(&[&parent], b"\xff subject\n")).unwrap();
    assert_eq!(row.subject, None);
    assert_eq!(row.parents[0].oid, parent);
    assert!(!row.root);
}

#[test]
fn malformed_truncated_duplicate_and_unrequested_batch_output_never_becomes_a_graph() {
    let requested = vec![hash('d')];
    let body = commit(&[], b"root\n");
    let valid = format!("{} commit {}\n{}\n", requested[0], body.len(), String::from_utf8(body.clone()).unwrap());
    assert!(reader::parse_batch(valid.as_bytes(), &requested).unwrap()[0].root);
    for malformed in [
        format!("{} commit {}\n{}", requested[0], body.len(), String::from_utf8(body).unwrap()),
        format!("{} commit 9999999\nshort\n", requested[0]),
        format!("{} blob 5\nshort\n", requested[0]),
        valid.replace(&requested[0], &hash('e')),
        format!("{valid}{valid}"),
    ] {
        assert_eq!(reader::parse_batch(malformed.as_bytes(), &requested), Err(HistoryErrorCode::InvalidOutput));
    }
    assert_eq!(reader::parse_batch(format!("{} missing\n", requested[0]).as_bytes(), &requested), Err(HistoryErrorCode::MissingObjects));
    assert_eq!(reader::parse_traversal(format!("{0}\n{0}\n", requested[0]).as_bytes()), Err(HistoryErrorCode::InvalidOutput));
}

#[test]
fn full_sha256_oids_are_accepted_but_options_and_abbreviations_are_not_authority() {
    assert_eq!(reader::oid("a".repeat(64).as_bytes()).unwrap().len(), 64);
    assert_eq!(reader::oid(b"abcd1234"), Err(HistoryErrorCode::InvalidOutput));
    assert_eq!(reader::oid(b"--all"), Err(HistoryErrorCode::InvalidOutput));
    assert_eq!(reader::parse_refs(format!("refs/heads/main\0{}\0commit\0\n", hash('a')).as_bytes()).unwrap()[0].name, "main");
    assert_eq!(reader::parse_refs(format!("refs/heads/unsafe name\0{}\0commit\0\n", hash('a')).as_bytes()), Err(HistoryErrorCode::InvalidOutput));
}

fn candidate(entry: &str) -> HistoryCandidate {
    let head = HistoryHead { scope: HeadScope::Worktree, state: HeadState::Attached, branch: Some("main".into()), oid: Some(hash('a')) };
    HistoryCandidate { snapshot: Arc::new(PinnedSnapshot { upstream: UpstreamSummary::empty(UpstreamState::NoUpstream, None), refs: Vec::new(), head: head.clone(), seeds: vec![hash('a')], shallow: false }),
        page: HistoryPage { entry_id: entry.into(), cursor: None, commits: Vec::new(), refs: Vec::new(), upstream: UpstreamSummary::empty(UpstreamState::NoUpstream, None), head, has_more: true, completeness: Completeness::Paged } }
}

#[tokio::test]
async fn cursor_authority_expires_and_refresh_or_reselection_invalidates_older_completions() {
    tokio::time::pause();
    let controller = HistoryController::default();
    let first = controller.begin("entry", 1, None, None).unwrap();
    let HistoryPageResult::Page { page } = controller.publish(first, candidate("entry")) else { panic!("first page must publish") };
    let cursor = page.cursor.unwrap();
    assert!(matches!(controller.begin("other", 1, Some(&cursor), None), Err(HistoryErrorCode::StaleCursor)));
    assert!(matches!(controller.begin("entry", 2, Some(&cursor), None), Err(HistoryErrorCode::StaleCursor)));
    let pending = controller.begin("entry", 1, Some(&cursor), None).unwrap();
    controller.begin("entry", 1, None, None).unwrap();
    assert!(matches!(controller.publish(pending, candidate("entry")), HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    let first = controller.begin("entry", 1, None, None).unwrap();
    let HistoryPageResult::Page { page } = controller.publish(first, candidate("entry")) else { panic!("fresh page must publish") };
    tokio::time::advance(Duration::from_secs(301)).await;
    assert!(matches!(controller.begin("entry", 1, page.cursor.as_deref(), None), Err(HistoryErrorCode::StaleCursor)));
}

#[test]
fn generated_octopus_commits_preserve_raw_parent_order_for_both_object_formats() {
    for width in [40, 64] {
        for parent_count in 0..=8 {
            let oid = "d".repeat(width);
            let parents: Vec<_> = (1..=parent_count).map(|index| format!("{index:0width$x}")).collect();
            let mut bytes = format!("tree {}\n", "a".repeat(width)).into_bytes();
            for parent in parents.iter().rev() {
                bytes.extend_from_slice(format!("parent {parent}\n").as_bytes());
            }
            bytes.extend_from_slice(b"author Fixture <fixture@example.org> 1 +0000\ncommitter Fixture <fixture@example.org> 1 +0000\n\nsubject\n");
            // Header-looking message text must never create ancestry.
            bytes.extend_from_slice(format!("parent {}\n", "e".repeat(width)).as_bytes());

            let row = reader::parse_commit(&oid, &bytes).unwrap();

            assert_eq!(row.oid, oid);
            assert_eq!(row.subject.as_deref(), Some("subject"));
            assert_eq!(row.root, parent_count == 0);
            assert_eq!(row.parents.iter().map(|parent| parent.oid.as_str()).collect::<Vec<_>>(),
                parents.iter().rev().map(String::as_str).collect::<Vec<_>>());
            assert!(row.parents.iter().all(|parent| parent.state == ParentState::OutsidePage));
        }
    }
}

#[test]
fn generated_batch_frames_use_byte_lengths_and_reject_every_incomplete_suffix() {
    let root = hash('b');
    let child = hash('c');
    let requested = vec![root.clone(), child.clone()];
    let root_body = commit(&[], b"root\n");
    let prefix = format!("{root} commit {}\n{}\n", root_body.len(), String::from_utf8(root_body).unwrap());
    for subject in ["", "é — subject", "line\r", "tree and parent are only text"] {
        let message = format!("{subject}\n\n{root} commit 0\n\0body\n");
        let body = commit(&[&root], message.as_bytes());
        let mut suffix = format!("{child} commit {}\n", body.len()).into_bytes();
        suffix.extend_from_slice(&body);
        suffix.push(b'\n');
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(&suffix);

        let rows = reader::parse_batch(&bytes, &requested).unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].oid, root);
        assert!(rows[0].root);
        assert_eq!(rows[1].oid, child);
        assert_eq!(rows[1].subject.as_deref(), Some(subject));
        assert_eq!(rows[1].parents, vec![HistoryParent { oid: root.clone(), state: ParentState::OutsidePage }]);
        for retained in 0..suffix.len() {
            assert_eq!(reader::parse_batch(&bytes[..prefix.len() + retained], &requested),
                Err(HistoryErrorCode::InvalidOutput), "subject {subject:?}, retained {retained}");
        }
        assert_eq!(reader::parse_batch(&bytes, &[child.clone(), root.clone()]), Err(HistoryErrorCode::InvalidOutput));
    }
}

#[test]
fn concurrent_cursor_completions_consume_authority_once_without_revoking_the_winner() {
    let controller = HistoryController::default();
    let first = controller.begin("entry", 1, None, Some("topic")).unwrap();
    let HistoryPageResult::Page { page } = controller.publish(first, candidate("entry")) else { panic!("first page must publish") };
    let cursor = page.cursor.unwrap();
    assert!(matches!(controller.begin("entry", 1, Some(&cursor), Some("main")), Err(HistoryErrorCode::StaleCursor)));
    assert!(matches!(controller.begin("entry", 1, Some(&cursor), None), Err(HistoryErrorCode::StaleCursor)));
    let winner = controller.begin("entry", 1, Some(&cursor), Some("topic")).unwrap();
    let duplicate = controller.begin("entry", 1, Some(&cursor), Some("topic")).unwrap();

    let HistoryPageResult::Page { page } = controller.publish(winner, candidate("entry")) else { panic!("continuation must publish") };
    let next_cursor = page.cursor.unwrap();
    assert_ne!(next_cursor, cursor);
    assert!(matches!(controller.publish(duplicate, candidate("entry")),
        HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    assert!(matches!(controller.begin("entry", 1, Some(&cursor), Some("topic")), Err(HistoryErrorCode::StaleCursor)));

    let final_ticket = controller.begin("entry", 1, Some(&next_cursor), Some("topic")).unwrap();
    let mut last = candidate("entry");
    last.page.has_more = false;
    last.page.completeness = Completeness::Complete;
    let HistoryPageResult::Page { page } = controller.publish(final_ticket, last) else { panic!("winner's continuation must remain valid") };
    assert_eq!(page.cursor, None);
    assert!(!page.has_more);
    assert!(matches!(controller.begin("entry", 1, Some(&next_cursor), Some("topic")), Err(HistoryErrorCode::StaleCursor)));
}

#[tokio::test]
async fn visible_upstream_ranges_survive_cursor_idle_expiry_but_not_refresh() {
    tokio::time::pause();
    let controller = HistoryController::default();
    let range = UpstreamRange { token: Uuid::new_v4().to_string(), base_oid: hash('a'), tip_oid: hash('b') };
    let mut result = candidate("entry");
    result.page.upstream.outgoing = Some(range.clone());
    Arc::get_mut(&mut result.snapshot).unwrap().upstream = result.page.upstream.clone();
    let ticket = controller.begin("entry", 1, None, None).unwrap();
    controller.publish(ticket, result);
    tokio::time::advance(Duration::from_secs(301)).await;
    assert_eq!(controller.resolve_range("entry", 1, &range.token), Some(range.clone()));
    assert_eq!(controller.resolve_range("other", 1, &range.token), None);
    assert_eq!(controller.resolve_range("entry", 2, &range.token), None);
    controller.begin("entry", 1, None, None).unwrap();
    assert_eq!(controller.resolve_range("entry", 1, &range.token), None);
}
