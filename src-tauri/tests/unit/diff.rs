//! Covers complete-patch validation, newline semantics and resource/encoding boundaries.

use super::*;

fn patch(body: &str) -> Vec<u8> {
    format!("diff --git from to\nindex 1234567..abcdef0 100644\n--- from\n+++ to\n{body}").into_bytes()
}

#[test]
fn newline_markers_attach_to_each_endpoint_without_becoming_source_lines() {
    let hunks = parser::parse(&patch("@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"), ProbeDeadline::new()).unwrap();
    assert_eq!(hunks, vec![TextHunk {
        old_start: 1, old_count: 1, new_start: 1, new_count: 1,
        lines: vec![TextLine { kind: LineKind::Removal, text: "old".into(), no_final_newline: true },
            TextLine { kind: LineKind::Addition, text: "new".into(), no_final_newline: true }],
    }]);
}

#[test]
fn malformed_multifile_counts_and_terminal_markers_never_publish_partial_hunks() {
    for body in [
        "@@ -1,2 +1 @@\n-old\n+new\n",
        "@@ -1 +1 @@\n-old\n+new\ndiff --git other third\n",
        "@@ -1 +1 @@\n\\ No newline at end of file\n-old\n+new\n",
        "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n\\ No newline at end of file\n+new\n",
        "@@ -1,2 +1 @@\n-old\n\\ No newline at end of file\n-next\n+new\n",
        "@@ -1 +1 @@\n same\n",
        "@@ -4294967295,2 +1 @@\n-old\n-next\n+new\n",
        "@@ -1 +1 @@\n-old\n+new",
    ] {
        assert_eq!(parser::parse(&patch(body), ProbeDeadline::new()), Err(unavailable(ReviewErrorCode::InvalidOutput)), "{body:?}");
    }
}

#[test]
fn empty_endpoint_ranges_are_distinct_from_existing_empty_source_lines() {
    let hunks = parser::parse(&patch("@@ -0,0 +1,2 @@\n+\n+second\n"), ProbeDeadline::new()).unwrap();
    assert_eq!((hunks[0].old_start, hunks[0].old_count, hunks[0].new_count), (0, 0, 2));
    assert_eq!(hunks[0].lines[0].text, "");
    assert_eq!(hunks[0].lines[1].text, "second");
}

#[test]
fn content_and_patch_bounds_do_not_masquerade_as_empty_text() {
    assert_eq!(validate_content(b"\0binary"), Err(unsupported(UnsupportedReason::Binary)));
    assert_eq!(validate_content(b"\xff"), Err(unsupported(UnsupportedReason::UnsupportedEncoding)));
    assert_eq!(validate_content(&vec![b'x'; CONTENT_LIMIT + 1]), Err(unsupported(UnsupportedReason::LargeOrTruncated)));
    assert_eq!(parser::parse(&vec![b'x'; CONTENT_LIMIT + 1], ProbeDeadline::new()), Err(unsupported(UnsupportedReason::LargeOrTruncated)));
    assert_eq!(parser::parse(&patch("@@ -0,0 +1 @@\n+\u{fffd}\n"), ProbeDeadline::new()).unwrap()[0].lines[0].text, "\u{fffd}");
}

#[test]
fn git_trailing_hunk_context_is_metadata_not_an_invalid_range_or_source_line() {
    let body = "@@ -12,7 +12,7 @@ preceding function or source label\n context 12\n context 13\n context 14\n-old 15\n+new 15\n context 16\n context 17\n context 18\n";
    let hunks = parser::parse(&patch(body), ProbeDeadline::new()).unwrap();
    assert_eq!((hunks[0].old_start, hunks[0].old_count, hunks[0].new_start, hunks[0].new_count), (12, 7, 12, 7));
    assert_eq!(hunks[0].lines[3].text, "old 15");
    assert_eq!(hunks[0].lines[4].text, "new 15");
    assert_eq!(hunks[0].lines.len(), 8);
    assert_eq!(parser::parse(&patch("@@ -1 +1 @@invalid label\n-old\n+new\n"), ProbeDeadline::new()), Err(unavailable(ReviewErrorCode::InvalidOutput)));
}

#[test]
fn generated_hunks_preserve_both_endpoint_counts_and_literal_source_text() {
    for context_count in 0..=2u32 {
        for removed in 0..=4u32 {
            for added in 0..=4u32 {
                if removed == 0 && added == 0 { continue; }
                let old_count = context_count + removed;
                let new_count = context_count + added;
                let old_start = if old_count == 0 { 10 } else { 11 };
                let new_start = if new_count == 0 { 10 } else { 11 };
                let mut body = format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@\n");
                body.push_str(&" @@ literal context\r\n".repeat(context_count as usize));
                body.push_str(&"--literal removal é\n".repeat(removed as usize));
                body.push_str(&"++literal addition \t\n".repeat(added as usize));

                let hunks = parser::parse(&patch(&body), ProbeDeadline::new()).unwrap();

                assert_eq!(hunks.len(), 1, "{body:?}");
                let hunk = &hunks[0];
                assert_eq!((hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count),
                    (old_start, old_count, new_start, new_count), "{body:?}");
                let old_lines: Vec<_> = hunk.lines.iter().filter(|line| line.kind != LineKind::Addition)
                    .map(|line| line.text.as_str()).collect();
                let new_lines: Vec<_> = hunk.lines.iter().filter(|line| line.kind != LineKind::Removal)
                    .map(|line| line.text.as_str()).collect();
                let expected_old: Vec<_> = std::iter::repeat_n("@@ literal context\r", context_count as usize)
                    .chain(std::iter::repeat_n("-literal removal é", removed as usize)).collect();
                let expected_new: Vec<_> = std::iter::repeat_n("@@ literal context\r", context_count as usize)
                    .chain(std::iter::repeat_n("+literal addition \t", added as usize)).collect();
                assert_eq!(old_lines, expected_old, "{body:?}");
                assert_eq!(new_lines, expected_new, "{body:?}");
                assert!(hunk.lines.iter().all(|line| !line.no_final_newline));
            }
        }
    }
}

#[test]
fn overlapping_later_hunks_reject_the_whole_patch_on_either_endpoint() {
    for count in 1..=4u32 {
        let first = format!("@@ -10,{count} +10,{count} @@\n{}{}",
            "-old\n".repeat(count as usize), "+new\n".repeat(count as usize));
        let adjacent = format!("{first}@@ -{} +{} @@\n-next old\n+next new\n", 10 + count, 10 + count);
        let hunks = parser::parse(&patch(&adjacent), ProbeDeadline::new()).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!((hunks[1].old_start, hunks[1].new_start), (10 + count, 10 + count));
        for overlap in 0..count {
            for (old_start, new_start) in [(10 + overlap, 10 + count), (10 + count, 10 + overlap)] {
                let body = format!("{first}@@ -{old_start} +{new_start} @@\n-next old\n+next new\n");
                assert_eq!(parser::parse(&patch(&body), ProbeDeadline::new()),
                    Err(unavailable(ReviewErrorCode::InvalidOutput)), "{body:?}");
            }
        }
    }
}
