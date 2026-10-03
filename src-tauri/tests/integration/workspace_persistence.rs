//! Exercises the private workspace format, bounded I/O and complete replacement on isolated paths.

use std::fs;
use std::path::{Path, PathBuf};

use super::{load, save, PersistenceErrorCode, SavedRepository, WorkspaceDocument, DOCUMENT_LIMIT};

fn document(roots: &[PathBuf], active_root: Option<PathBuf>) -> WorkspaceDocument {
    WorkspaceDocument {
        version: 1,
        repositories: roots.iter().map(|root| SavedRepository {
            root: root.clone(),
            display_name: None,
        }).collect(),
        active_root,
    }
}

fn document_of_size(directory: &Path, size: usize) -> WorkspaceDocument {
    let prefix = directory.join("repository-").to_str().unwrap().to_owned();
    let mut document = document(&[PathBuf::from(&prefix)], None);
    let initial_size = serde_json::to_vec(&document).unwrap().len();
    document.repositories[0].root = PathBuf::from(format!("{prefix}{}", "x".repeat(size - initial_size)));
    assert_eq!(serde_json::to_vec(&document).unwrap().len(), size);
    document
}

#[tokio::test]
async fn missing_file_is_an_empty_first_launch_without_creating_storage() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("not-created").join("workspace.json");
    assert_eq!(load(&path).await.unwrap(), WorkspaceDocument::default());
    assert!(!path.parent().unwrap().exists());
}

#[tokio::test]
async fn saves_only_ordered_roots_and_selection_and_replaces_the_complete_document() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("app-data").join("workspace.json");
    let first_root = temp.path().join("café-project");
    let second_root = temp.path().join("linked context");
    let first = document(&[second_root.clone(), first_root.clone()], Some(first_root.clone()));
    save(&path, &first).await.unwrap();
    let encoded: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(encoded, serde_json::json!({
        "version": 1,
        "repositories": [{ "root": second_root }, { "root": first_root }],
        "activeRoot": first_root,
    }));
    assert_eq!(load(&path).await.unwrap(), first);

    let next = document(&[first_root], None);
    save(&path, &next).await.unwrap();
    assert_eq!(load(&path).await.unwrap(), next);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[tokio::test]
async fn malformed_documents_fail_whole_and_preserve_the_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    let root = temp.path().join("repository");
    let root = root.to_str().unwrap();
    let other = temp.path().join("outside-workspace");
    let mut malformed = vec![
        b"".to_vec(),
        b"{broken json".to_vec(),
        b"{\"version\":1,\"repositories\":[]}".to_vec(),
        b"{\"version\":1,\"activeRoot\":null}".to_vec(),
        b"{\"repositories\":[],\"activeRoot\":null}".to_vec(),
        b"{\"version\":1,\"version\":1,\"repositories\":[],\"activeRoot\":null}".to_vec(),
        b"{\"version\":1.5,\"repositories\":[],\"activeRoot\":null}".to_vec(),
        b"{\"version\":\"1\",\"repositories\":[],\"activeRoot\":null}".to_vec(),
        b"{\"version\":1,\"repositories\":null,\"activeRoot\":null}".to_vec(),
    ];
    for value in [
        serde_json::json!({ "version": 1, "repositories": [{}], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": "" }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": "relative/project" }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": format!("{root}\0") }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": root }, { "root": root }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": root }], "activeRoot": other }),
        serde_json::json!({ "version": 1, "repositories": [], "activeRoot": root }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": root, "head": "main" }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": root, "displayName": "   " }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [{ "root": root, "displayName": 12 }], "activeRoot": null }),
        serde_json::json!({ "version": 1, "repositories": [], "activeRoot": null, "head": "main" }),
    ] {
        malformed.push(serde_json::to_vec(&value).unwrap());
    }
    // Include a valid first location: invalid later entries must not partially restore it.
    malformed.push(serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "repositories": [{ "root": root }, { "root": "relative" }],
        "activeRoot": root,
    })).unwrap());
    for bytes in malformed {
        fs::write(&path, &bytes).unwrap();
        assert_eq!(load(&path).await.unwrap_err().code, PersistenceErrorCode::LoadFailed);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[tokio::test]
async fn unsupported_versions_are_distinct_from_corruption_and_remain_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    // A future schema need not retain the version-one fields.
    for bytes in [b"{\"version\":2,\"futureChoices\":[1]}".as_slice(), b"{\"version\":0}".as_slice()] {
        fs::write(&path, bytes).unwrap();
        assert_eq!(load(&path).await.unwrap_err().code, PersistenceErrorCode::UnsupportedVersion);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[tokio::test]
async fn exact_limit_round_trips_and_oversized_saves_preserve_the_previous_document() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    for size in [DOCUMENT_LIMIT - 1, DOCUMENT_LIMIT] {
        let document = document_of_size(temp.path(), size);
        save(&path, &document).await.unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), size as u64);
        assert_eq!(load(&path).await.unwrap(), document);
    }
    let previous_bytes = fs::read(&path).unwrap();
    let oversized = document_of_size(temp.path(), DOCUMENT_LIMIT + 1);
    assert_eq!(save(&path, &oversized).await.unwrap_err().code, PersistenceErrorCode::SaveFailed);
    assert_eq!(fs::read(&path).unwrap(), previous_bytes);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn oversized_loads_are_rejected_without_truncating_or_rewriting_them() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    let document = document_of_size(temp.path(), DOCUMENT_LIMIT + 1);
    let bytes = serde_json::to_vec(&document).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert_eq!(load(&path).await.unwrap_err().code, PersistenceErrorCode::LoadFailed);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[tokio::test]
async fn invalid_saves_preserve_the_previous_complete_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    let root = temp.path().join("repository");
    let previous = document(&[root.clone()], Some(root.clone()));
    save(&path, &previous).await.unwrap();
    let previous_bytes = fs::read(&path).unwrap();
    let mut unsupported = WorkspaceDocument::default();
    unsupported.version = 2;
    for invalid in [
        unsupported,
        document(&[PathBuf::from("relative")], None),
        document(&[root.clone(), root.clone()], None),
        document(&[root], Some(temp.path().join("not-admitted"))),
    ] {
        assert_eq!(save(&path, &invalid).await.unwrap_err().code, PersistenceErrorCode::SaveFailed);
        assert_eq!(fs::read(&path).unwrap(), previous_bytes);
    }
}

#[tokio::test]
async fn replacement_failure_keeps_the_destination_and_cleans_up_the_temporary_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    // A nonempty directory forces portable replacement failure without relying on user permissions.
    fs::create_dir(&path).unwrap();
    let previous = path.join("preserved.json");
    fs::write(&previous, b"previous complete bytes").unwrap();
    assert_eq!(save(&path, &WorkspaceDocument::default()).await.unwrap_err().code, PersistenceErrorCode::SaveFailed);
    assert_eq!(fs::read(&previous).unwrap(), b"previous complete bytes");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn unavailable_parent_reports_save_failure_without_mutating_it() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("not-a-directory");
    fs::write(&parent, b"preserved bytes").unwrap();
    assert_eq!(save(&parent.join("workspace.json"), &WorkspaceDocument::default()).await.unwrap_err().code, PersistenceErrorCode::SaveFailed);
    assert_eq!(fs::read(&parent).unwrap(), b"preserved bytes");
}

#[cfg(unix)]
#[tokio::test]
async fn saved_choices_are_owner_private_even_when_replacing_an_existing_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    fs::write(&path, b"previous bytes").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    save(&path, &WorkspaceDocument::default()).await.unwrap();
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_native_roots_cannot_lossily_replace_saved_choices() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    save(&path, &WorkspaceDocument::default()).await.unwrap();
    let previous_bytes = fs::read(&path).unwrap();
    let root = temp.path().join(OsString::from_vec(vec![b'r', 0xff]));
    assert_eq!(save(&path, &document(&[root], None)).await.unwrap_err().code, PersistenceErrorCode::SaveFailed);
    assert_eq!(fs::read(&path).unwrap(), previous_bytes);
}
