//! Exercises portable bounded status parsing without filesystem or Git I/O.

use std::path::PathBuf;

use super::*;

const OID: &str = "1111111111111111111111111111111111111111";

const ZERO_OID: &str = "0000000000000000000000000000000000000000";
fn ordinary(xy: &str, path: &str) -> Vec<u8> {
    format!("1 {xy} N... 100644 100644 100644 {OID} {OID} {path}\0").into_bytes()
}

fn parse(bytes: &[u8]) -> Result<Vec<StatusPath>, StatusError> {
    parse_status(bytes, ProbeDeadline::new())
}

fn path<'a>(paths: &'a [StatusPath], name: &str) -> &'a StatusPath {
    paths.iter().find(|path| path.display_path == name).unwrap()
}

#[test]
fn ordinary_categories_preserve_both_sides_of_partial_staging() {
    let mut bytes = ordinary("MM", "partial.txt");
    bytes.extend(ordinary("A.", "added.txt"));
    bytes.extend(ordinary(".D", "deleted.txt"));
    bytes.extend(ordinary("D.", "removed.txt"));
    bytes.extend(ordinary(".A", "intent.txt"));
    let paths = parse(&bytes).unwrap();
    assert_eq!(path(&paths, "partial.txt").staged, Some(ChangeKind::Modified));
    assert_eq!(path(&paths, "partial.txt").unstaged, Some(ChangeKind::Modified));
    assert_eq!(path(&paths, "added.txt").staged, Some(ChangeKind::Added));
    assert_eq!(path(&paths, "deleted.txt").unstaged, Some(ChangeKind::Deleted));
    assert_eq!(path(&paths, "removed.txt").staged, Some(ChangeKind::Deleted));
    assert_eq!(path(&paths, "intent.txt").unstaged, Some(ChangeKind::Added));
}

#[test]
fn untracked_native_segments_preserve_spaces_newlines_and_unicode() {
    let paths = parse(b"? nested/space name\n\xc3\xa9.txt\0").unwrap();
    assert_eq!(paths[0].native_path, PathBuf::from("nested/space name\né.txt"));
    assert_eq!(paths[0].segments, ["nested", "space name\né.txt"]);
    assert!(paths[0].untracked);
    assert_eq!(paths[0].staged, None);
    assert_eq!(paths[0].unstaged, None);
}

#[test]
fn rename_consumes_its_origin_without_treating_it_as_another_record() {
    let mut bytes = format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 new name\0old\nname\0").into_bytes();
    bytes.extend(b"? next\0");
    let paths = parse(&bytes).unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0].native_origin, Some(PathBuf::from("old\nname")));
    assert_eq!(paths[0].unsupported_kind, Some(UnsupportedKind::RenameOrCopy));
    assert_eq!(paths[0].staged, None);
    assert_eq!(paths[0].unstaged, None);
    assert_eq!(paths[1].native_path, PathBuf::from("next"));
}

#[test]
fn copy_keeps_its_source_and_is_not_an_ordinary_added_comparison() {
    let paths = parse(format!("2 C. N... 100644 100644 100644 {OID} {OID} C075 copy\0original\0").as_bytes()).unwrap();
    assert_eq!(paths[0].native_origin, Some(PathBuf::from("original")));
    assert_eq!(paths[0].unsupported_kind, Some(UnsupportedKind::RenameOrCopy));
    assert_eq!(paths[0].staged, None);
}

#[test]
fn conflicts_keep_stage_modes_without_inventing_staged_or_unstaged_comparisons() {
    let paths = parse(format!("u UU N... 100644 100755 100644 100644 {OID} {OID} {OID} conflict\0").as_bytes()).unwrap();
    assert!(paths[0].conflict);
    assert_eq!(paths[0].staged, None);
    assert_eq!(paths[0].unstaged, None);
    assert_eq!(paths[0].modes.as_ref().unwrap().conflict_stages, Some([0o100644, 0o100755, 0o100644]));
}

#[test]
fn absent_git_modes_require_absent_object_ids_and_preserve_real_add_delete_records() {
    let bytes = format!(
        "1 A. N... 000000 100644 100644 {ZERO_OID} {OID} added\0\
         1 D. N... 100644 000000 000000 {OID} {ZERO_OID} deleted\0\
         1 .A N... 000000 000000 100644 {ZERO_OID} {ZERO_OID} intent\0"
    );
    let paths = parse(bytes.as_bytes()).unwrap();
    assert_eq!(path(&paths, "added").staged, Some(ChangeKind::Added));
    assert_eq!(path(&paths, "deleted").staged, Some(ChangeKind::Deleted));
    assert_eq!(path(&paths, "intent").unstaged, Some(ChangeKind::Added));
    let invalid = format!("1 A. N... 000000 100644 100644 {OID} {OID} added\0");
    assert_eq!(parse(invalid.as_bytes()), Err(StatusError::InvalidStatus));
    let invalid = format!("u UU N... 100644 100644 000000 100644 {OID} {OID} {OID} conflict\0");
    assert_eq!(parse(invalid.as_bytes()), Err(StatusError::InvalidStatus));
}

#[test]
fn submodules_and_type_changes_are_visible_but_not_ordinary_comparisons() {
    let bytes = format!("1 .M S.MU 160000 160000 160000 {OID} {OID} module\0\
                         1 .T N... 100644 100644 120000 {OID} {OID} link\0");
    let paths = parse(bytes.as_bytes()).unwrap();
    assert_eq!(path(&paths, "module").unsupported_kind, Some(UnsupportedKind::Submodule));
    assert_eq!(path(&paths, "link").unsupported_kind, Some(UnsupportedKind::TypeChange));
    assert_eq!(path(&paths, "module").unstaged, None);
    assert_eq!(path(&paths, "link").unstaged, None);
    assert_eq!(path(&paths, "link").modes.as_ref().unwrap().worktree, 0o120000);
}

#[test]
fn sha256_object_ids_are_supported_but_mixed_object_formats_are_invalid() {
    let oid = "2".repeat(64);
    assert!(parse(format!("1 .M N... 100644 100644 100644 {oid} {oid} file\0").as_bytes()).is_ok());
    assert_eq!(parse(format!("1 .M N... 100644 100644 100644 {OID} {oid} file\0").as_bytes()), Err(StatusError::InvalidStatus));
}

#[test]
fn malformed_records_never_return_partial_or_false_clean_results() {
    let invalid = [
        b"? file".as_slice(), b"\0", b"! ignored\0", b"# branch.head main\0",
        b"? \0", b"? /absolute\0", b"? ../escape\0", b"? a/../escape\0",
        b"? a/./file\0", b"? a//file\0", b"? a/\0", b"? duplicate\0? duplicate\0",
        b"1 MM N... 100644\0", b"3 unknown\0", b"? valid\0bad\0",
    ];
    for bytes in invalid {
        assert_eq!(parse(bytes), Err(StatusError::InvalidStatus), "{bytes:?}");
    }
    for xy in ["..", "UX", "UU", "R.", ".C", "MZ", "M", "MMM"] {
        assert_eq!(parse(&ordinary(xy, "file")), Err(StatusError::InvalidStatus));
    }
    for field in ["N..X", "SXYZ", "S..."] {
        let bytes = format!("1 .M {field} 100644 100644 100644 {OID} {OID} file\0");
        assert_eq!(parse(bytes.as_bytes()), Err(StatusError::InvalidStatus));
    }
    for mode in ["100600", "999999", "10064", "0100644"] {
        let bytes = format!("1 .M N... {mode} 100644 100644 {OID} {OID} file\0");
        assert_eq!(parse(bytes.as_bytes()), Err(StatusError::InvalidStatus));
    }
    assert_eq!(parse(format!("1 .M N... 100644 100644 100644 nope {OID} file\0").as_bytes()), Err(StatusError::InvalidStatus));
    assert_eq!(parse(format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 file\0").as_bytes()), Err(StatusError::InvalidStatus));
    assert_eq!(parse(format!("2 R. N... 100644 100644 100644 {OID} {OID} R101 file\0origin\0").as_bytes()), Err(StatusError::InvalidStatus));
    assert_eq!(parse(format!("2 R. N... 100644 100644 100644 {OID} {OID} C100 file\0origin\0").as_bytes()), Err(StatusError::InvalidStatus));
    assert_eq!(parse(format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 file\0../origin\0").as_bytes()), Err(StatusError::InvalidStatus));
    assert_eq!(parse(format!("u MM N... 100644 100644 100644 100644 {OID} {OID} {OID} file\0").as_bytes()), Err(StatusError::InvalidStatus));
}

#[test]
fn non_utf8_output_is_unavailable_instead_of_lossy_path_identity() {
    assert_eq!(parse(b"? invalid-\xff\0"), Err(StatusError::UnsupportedPathEncoding));
    assert_eq!(parse(format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 valid\0").as_bytes().iter().copied().chain(b"invalid-\xff\0".iter().copied()).collect::<Vec<_>>().as_slice()), Err(StatusError::UnsupportedPathEncoding));
}

#[test]
fn bounded_parser_rejects_oversized_output_and_excessive_path_counts() {
    assert_eq!(parse(&vec![b'x'; STATUS_OUTPUT_LIMIT + 1]), Err(StatusError::ResourceLimit));
    let mut bytes = Vec::new();
    for index in 0..=STATUS_PATH_LIMIT {
        bytes.extend(format!("? {index}\0").as_bytes());
    }
    assert_eq!(parse(&bytes), Err(StatusError::ResourceLimit));
}

#[test]
fn generated_literal_paths_keep_exact_identity_across_record_kinds() {
    for depth in 0..=3 {
        for name in [" leading", "trailing ", "tab\tname", "line\nname", "é", "e\u{301}", "-option", ".hidden"] {
            let destination = format!("{}{}", "nested/".repeat(depth), name);
            let origin = format!("original/{destination}");
            let rename = format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 {destination}\0{origin}\0");
            for (record, expected_origin) in [
                (ordinary("MM", &destination), None),
                (format!("? {destination}\0").into_bytes(), None),
                (rename.into_bytes(), Some(origin.as_bytes())),
            ] {
                let mut bytes = record;
                bytes.extend_from_slice(b"? following\0");

                let paths = parse(&bytes).unwrap();

                assert_eq!(paths.len(), 2, "{destination:?}");
                assert_eq!(paths[0].native_path.as_os_str().as_encoded_bytes(), destination.as_bytes());
                assert_eq!(paths[0].display_path, destination);
                assert_eq!(paths[0].segments.join("/"), destination);
                assert_eq!(paths[0].native_origin.as_ref().map(|path| path.as_os_str().as_encoded_bytes()), expected_origin);
                assert_eq!(paths[1].native_path, PathBuf::from("following"));
                assert!(paths[1].untracked);
            }
        }
    }
}

#[test]
fn generated_invalid_path_segments_and_cross_category_duplicates_discard_valid_prefixes() {
    for depth in 0..=3 {
        for segment in ["", ".", ".."] {
            let invalid = format!("{}{segment}/file", "nested/".repeat(depth));
            let mut ordinary_tail = ordinary("MM", "valid");
            ordinary_tail.extend(ordinary(".M", &invalid));
            assert_eq!(parse(&ordinary_tail), Err(StatusError::InvalidStatus), "{invalid:?}");

            let mut origin_tail = ordinary("MM", "valid");
            origin_tail.extend(format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 renamed\0{invalid}\0").as_bytes());
            assert_eq!(parse(&origin_tail), Err(StatusError::InvalidStatus), "{invalid:?}");
        }
        let duplicate = format!("{}same", "nested/".repeat(depth));
        let mut bytes = ordinary(".M", &duplicate);
        bytes.extend(format!("? {duplicate}\0").as_bytes());
        assert_eq!(parse(&bytes), Err(StatusError::InvalidStatus), "{duplicate:?}");
    }
}
