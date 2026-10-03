//! Exercises private temporary authority storage, transactional limits and owned cleanup.

use super::*;

fn entry(path: &str) -> NativeEntry {
    NativeEntry { file: RepositoryFile {
        id: uuid::Uuid::new_v4().to_string(), display_path: path.to_owned(), segments: vec![path.to_owned()],
    }, kind: NativeFileKind::Regular, identity: None }
}

#[test]
fn storage_limit_rejects_the_whole_page_without_revoking_earlier_authority() {
    let mut authority = Authority::new().unwrap();
    let first = entry("first");
    authority.issue(std::slice::from_ref(&first), ProbeDeadline::new()).unwrap();
    let pages: i64 = authority.connection.query_row("PRAGMA page_count", [], |row| row.get(0)).unwrap();
    authority.connection.pragma_update(None, "max_page_count", pages).unwrap();
    let attempted = [entry("small"), entry(&"large".repeat(4096))];
    assert_eq!(authority.issue(&attempted, ProbeDeadline::new()).err(), Some(HistoryErrorCode::ResourceLimit));
    assert_eq!(authority.resolve(&first.file.id).unwrap().unwrap().file.display_path, "first");
    assert!(authority.resolve(&attempted[0].file.id).unwrap().is_none());
    assert!(authority.resolve(&attempted[1].file.id).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn authority_directory_and_database_are_private_and_removed_on_drop() {
    use std::os::unix::fs::PermissionsExt;
    let authority = Authority::new().unwrap();
    let directory = authority._directory.path().to_owned();
    assert_eq!(std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(directory.join("authority.sqlite")).unwrap().permissions().mode() & 0o777, 0o600);
    drop(authority);
    assert!(!directory.exists());
}
