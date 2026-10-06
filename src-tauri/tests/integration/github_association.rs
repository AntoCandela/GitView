//! Exercises association discovery without changing local Git state.
use super::*;
use crate::{
    github::{model::GithubHost, transport::GhReadAdapter},
    test_support::{executable, git, git_output, working_tree},
    workspace::NativeIdentity,
};
use std::path::Path;

fn context(root: &Path) -> SelectedContext {
    let root = root.canonicalize().unwrap();
    let git_dir = root.join(".git");
    SelectedContext {
        entry_id: "fixture".into(),
        identity: NativeIdentity::capture(&root, &git_dir).unwrap(),
        root,
        git_dir,
        kind: crate::git::RepositoryKind::WorkingTree,
    }
}
fn adapter(directory: &Path) -> GhReadAdapter {
    adapter_mode(directory, "normal")
}
fn adapter_mode(directory: &Path, mode: &str) -> GhReadAdapter {
    let program = executable(
        directory,
        r#"
case "$1" in
 version) printf '%s' 'gh version 2.81.0';;
 auth) printf '%s' '{"hosts":{"github.com":[{"host":"github.com","active":true,"state":"success"}]}}';;
 api)
 if [ "$GITVIEW_TEST_MODE" = failure ]; then
 case "$2" in repos/upstream/project/pulls*) printf 'HTTP/2.0 404 Not Found\r\nContent-Type: application/json\r\n\r\n{}'; exit 1;; esac
 fi
 case "$2" in
 graphql) cat >/dev/null; body='{"data":{"viewer":{"id":"viewer"}}}';;
 repos/fork/project) body='{"id":2,"name":"project","owner":{"login":"fork"},"fork":true,"parent":{"id":1,"name":"project","owner":{"login":"upstream"}}}';;
 repos/second/project) body='{"id":4,"name":"project","owner":{"login":"second"},"fork":false}';;
 repos/second/project/pulls*) body='[{"number":12,"title":"second base","state":"open","merged_at":null,"base":{"ref":"main","repo":{"id":4,"name":"project","owner":{"login":"second"}}},"head":{"ref":"published","repo":{"id":2,"name":"project","owner":{"login":"fork"}}}}]';;
 repos/upstream/project) body='{"id":1,"name":"project","owner":{"login":"upstream"},"fork":false}';;
 repos/upstream/project/pulls/9) body='{"number":9,"title":"known history","state":"closed","merged_at":"2025-01-01T00:00:00Z","base":{"ref":"main","repo":{"id":1,"name":"project","owner":{"login":"upstream"}}},"head":{"ref":null,"repo":null}}';;
 repos/upstream/project/pulls*) body='[{"number":7,"title":"correct","state":"open","merged_at":null,"base":{"ref":"main","repo":{"id":1,"name":"project","owner":{"login":"upstream"}}},"head":{"ref":"published","repo":{"id":2,"name":"project","owner":{"login":"fork"}}}},{"number":8,"title":"other fork","state":"open","merged_at":null,"base":{"ref":"main","repo":{"id":1,"name":"project","owner":{"login":"upstream"}}},"head":{"ref":"published","repo":{"id":3,"name":"project","owner":{"login":"other"}}}}]';;
 *) body='[]';;
 esac
 if [ "$GITVIEW_TEST_MODE" = malformed_repository ] && [ "$2" = repos/fork/project ]; then
 body='{"id":2,"name":"project","owner":{"login":"fork"}}'
 fi
 if [ "$GITVIEW_TEST_MODE" = incomplete ]; then
 case "$2" in repos/*/pulls*) printf 'HTTP/2.0 200 OK\r\nLink: <https://api.github.com/repos/upstream/project/pulls?page=2>; rel="next"\r\nContent-Type: application/json\r\n\r\n%s' "$body"; exit 0;; esac
 fi
 printf 'HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n%s' "$body";;
 *) exit 9;;
esac
"#,
    );
    GhReadAdapter::fixture(
        &program,
        vec![
            ("GITVIEW_TEST_MODE".into(), mode.into()),
            (
                "GH_CONFIG_DIR".into(),
                directory.join("config").into_os_string(),
            ),
            ("GH_TOKEN".into(), "fixture-token".into()),
        ],
    )
}
#[tokio::test]
async fn github_association_triangular_renamed_branch_matches_only_verified_push_head() {
    let (directory, root) = working_tree();
    git(&root, &["branch", "local-review"]);
    git(
        &root,
        &[
            "remote",
            "add",
            "upstream",
            "https://github.com/upstream/project.git",
        ],
    );
    git(
        &root,
        &["remote", "add", "fork", "git@github.com:fork/project.git"],
    );
    git(&root, &["config", "branch.local-review.remote", "upstream"]);
    git(
        &root,
        &["config", "branch.local-review.merge", "refs/heads/main"],
    );
    git(&root, &["config", "branch.local-review.pushRemote", "fork"]);
    git(
        &root,
        &[
            "config",
            "remote.fork.push",
            "refs/heads/local-review:refs/heads/published",
        ],
    );
    std::fs::write(root.join("tracked"), "staged").unwrap();
    git(&root, &["add", "tracked"]);
    std::fs::write(root.join("tracked"), "unstaged").unwrap();
    std::fs::write(root.join("untracked"), "untouched").unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let before = git_output(&root, &["show-ref"]).stdout;
    let config = std::fs::read(root.join(".git/config")).unwrap();
    let coordinator = Arc::new(PrDemandCoordinator::new(adapter(directory.path())));
    let account = coordinator.observe_account().await.unwrap();
    assert_eq!(account.host, GithubHost::GithubCom);
    let resolver = AssociationResolver::new(coordinator);
    let context = context(&root);
    let capture = resolver
        .capture(&context, Some("local-review"))
        .await
        .unwrap();
    let result = resolver
        .resolve(&context, capture, &account, None, &[])
        .await
        .unwrap();
    assert_eq!(result.state, ResolutionState::Single);
    assert!(result.complete);
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].identity.number, 7);
    assert_eq!(result.candidates[0].head_ref.as_deref(), Some("published"));
    assert_eq!(git_output(&root, &["show-ref"]).stdout, before);
    assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), config);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(root.join("tracked")).unwrap(), b"unstaged");
    assert_eq!(std::fs::read(root.join("untracked")).unwrap(), b"untouched");
    assert_eq!(
        git_output(&root, &["symbolic-ref", "--short", "HEAD"]).stdout,
        b"main\n"
    );
}

#[tokio::test]
async fn github_association_failed_or_incomplete_candidate_query_never_means_absence() {
    for mode in ["failure", "incomplete"] {
        let (directory, root) = working_tree();
        git(
            &root,
            &["remote", "add", "fork", "git@github.com:fork/project.git"],
        );
        git(&root, &["config", "branch.main.remote", "fork"]);
        git(&root, &["config", "branch.main.merge", "refs/heads/main"]);
        let coordinator = Arc::new(PrDemandCoordinator::new(adapter_mode(
            directory.path(),
            mode,
        )));
        let account = coordinator.observe_account().await.unwrap();
        let resolver = AssociationResolver::new(coordinator);
        let context = context(&root);
        let capture = resolver.capture(&context, None).await.unwrap();
        let result = resolver
            .resolve(&context, capture, &account, None, &[])
            .await
            .unwrap();
        assert!(!result.complete);
        assert_ne!(result.state, ResolutionState::None);
        if mode == "failure" {
            assert_eq!(
                result.failure.unwrap().code,
                crate::github::model::PrCode::RepositoryUnavailable
            );
        }
    }
}

#[tokio::test]
async fn github_association_effective_rewrites_aliases_and_local_dot_remain_explicit() {
    let (directory, root) = working_tree();
    git(&root, &["remote", "add", "origin", "work:fork/project.git"]);
    git(&root, &["config", "branch.main.remote", "origin"]);
    git(
        &root,
        &["config", "branch.main.merge", "refs/heads/published"],
    );
    git(&root, &["config", "push.default", "upstream"]);
    let resolver = AssociationResolver::new(Arc::new(PrDemandCoordinator::new(adapter(
        directory.path(),
    ))));
    let context = context(&root);
    let alias = resolver.capture(&context, None).await.unwrap();
    assert!(alias.unresolved);
    assert!(alias.heads.is_empty());
    git(
        &root,
        &["config", "url.https://github.com/.insteadOf", "work:"],
    );
    let rewritten = resolver.capture(&context, None).await.unwrap();
    assert!(!rewritten.unresolved);
    assert_eq!(rewritten.heads[0].repository.owner, "fork");
    assert_eq!(rewritten.heads[0].head_ref, "published");
    assert!(resolver.validate_capture(&context, &alias).await.is_err());
    git(&root, &["config", "branch.main.remote", "."]);
    let local = resolver.capture(&context, None).await.unwrap();
    assert!(local.unresolved);
    assert!(local.heads.is_empty());
}

#[tokio::test]
async fn github_association_explicit_alias_mapping_and_known_deleted_head_are_separate() {
    let (directory, root) = working_tree();
    git(
        &root,
        &["remote", "add", "origin", "git@work-alias:fork/project.git"],
    );
    let coordinator = Arc::new(PrDemandCoordinator::new(adapter(directory.path())));
    let account = coordinator.observe_account().await.unwrap();
    let resolver = AssociationResolver::new(coordinator);
    let context = context(&root);
    let capture = resolver.capture(&context, None).await.unwrap();
    let mapping = HeadMappingInput {
        owner: "fork".into(),
        repository: "project".into(),
        head_ref: "published".into(),
    };
    let known = KnownPull {
        identity: PrIdentity {
            host: "github.com".into(),
            base_repository_id: "1".into(),
            number: 9,
        },
        base_repository: crate::github::model::GithubRepository {
            id: "1".into(),
            host: GithubHost::GithubCom,
            owner: "upstream".into(),
            name: "project".into(),
            url: "https://github.com/upstream/project".into(),
        },
    };
    let result = resolver
        .resolve(
            &context,
            capture,
            &account,
            Some(&mapping),
            &[known.clone()],
        )
        .await
        .unwrap();
    assert_eq!(result.state, ResolutionState::Single);
    assert_eq!(result.candidates[0].identity.number, 7);
    assert_eq!(result.historical.len(), 1);
    assert!(matches!(result.historical[0].lifecycle, Lifecycle::Merged));
    assert!(result.historical[0].head_repository.is_none());
    git(&root, &["checkout", "--detach"]);
    let detached = resolver.capture(&context, None).await.unwrap();
    assert!(detached.branch_label.is_none());
    let history = resolver
        .resolve(&context, detached, &account, None, &[known])
        .await
        .unwrap();
    assert!(history.candidates.is_empty());
    assert_eq!(history.historical[0].identity.number, 9);
    assert_eq!(history.state, ResolutionState::Unresolved);
}

#[tokio::test]
async fn github_association_branchless_bare_and_reused_branch_preserve_context() {
    let (directory, root) = crate::test_support::unborn_working_tree();
    let resolver = AssociationResolver::new(Arc::new(PrDemandCoordinator::new(adapter(
        directory.path(),
    ))));
    let context = context(&root);
    let unborn = resolver.capture(&context, None).await.unwrap();
    assert!(unborn.heads.is_empty());
    crate::test_support::commit(&root);
    assert!(resolver.validate_capture(&context, &unborn).await.is_err());
    git(&root, &["branch", "reused"]);
    let old = resolver.capture(&context, Some("reused")).await.unwrap();
    git(&root, &["checkout", "reused"]);
    crate::test_support::commit(&root);
    let advanced = resolver.capture(&context, Some("reused")).await.unwrap();
    assert_eq!(old.config_fingerprint, advanced.config_fingerprint);
    assert!(resolver.validate_capture(&context, &old).await.is_err());
    git(&root, &["checkout", "main"]);
    git(&root, &["branch", "-D", "reused"]);
    assert!(resolver.validate_capture(&context, &old).await.is_err());
    crate::test_support::commit(&root);
    git(&root, &["branch", "reused"]);
    assert!(resolver.validate_capture(&context, &old).await.is_err());
    let bare = directory.path().join("bare");
    std::fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "-b", "main"]);
    let bare = bare.canonicalize().unwrap();
    let bare_context = SelectedContext {
        entry_id: "bare".into(),
        identity: NativeIdentity::capture(&bare, &bare).unwrap(),
        root: bare.clone(),
        git_dir: bare,
        kind: crate::git::RepositoryKind::Bare,
    };
    let capture = resolver.capture(&bare_context, None).await.unwrap();
    assert!(capture.heads.is_empty());
}

#[tokio::test]
async fn github_association_multiple_verified_bases_require_choice() {
    let (directory, root) = working_tree();
    git(
        &root,
        &["remote", "add", "origin", "git@github.com:fork/project.git"],
    );
    git(
        &root,
        &[
            "remote",
            "add",
            "second",
            "https://github.com/second/project.git",
        ],
    );
    git(&root, &["config", "branch.main.remote", "origin"]);
    git(
        &root,
        &["config", "branch.main.merge", "refs/heads/published"],
    );
    git(&root, &["config", "push.default", "upstream"]);
    let coordinator = Arc::new(PrDemandCoordinator::new(adapter(directory.path())));
    let account = coordinator.observe_account().await.unwrap();
    let resolver = AssociationResolver::new(coordinator);
    let context = context(&root);
    let capture = resolver.capture(&context, None).await.unwrap();
    let result = resolver
        .resolve(&context, capture, &account, None, &[])
        .await
        .unwrap();
    assert!(result.complete);
    assert_eq!(result.state, ResolutionState::Ambiguous);
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(result.bases.len(), 3);
}

#[tokio::test]
async fn github_association_missing_fork_evidence_cannot_certify_parent_scope() {
    let (directory, root) = working_tree();
    let coordinator = Arc::new(PrDemandCoordinator::new(adapter_mode(
        directory.path(),
        "malformed_repository",
    )));
    let account = coordinator.observe_account().await.unwrap();
    let resolver = AssociationResolver::new(coordinator);
    let context = context(&root);
    let capture = resolver.capture(&context, None).await.unwrap();
    let mapping = HeadMappingInput {
        owner: "fork".into(),
        repository: "project".into(),
        head_ref: "published".into(),
    };
    let result = resolver
        .resolve(&context, capture, &account, Some(&mapping), &[])
        .await;
    assert_eq!(
        result.unwrap_err().code,
        crate::github::model::PrCode::InvalidOutput
    );
}

#[tokio::test]
async fn github_association_push_url_and_wildcard_refspec_define_remote_head() {
    let (directory, root) = working_tree();
    git(&root, &["branch", "review/topic"]);
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/upstream/project.git",
        ],
    );
    git(
        &root,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "ssh://git@github.com/fork/project.git",
        ],
    );
    git(&root, &["config", "branch.review/topic.remote", "origin"]);
    git(
        &root,
        &["config", "branch.review/topic.merge", "refs/heads/main"],
    );
    git(
        &root,
        &[
            "config",
            "remote.origin.push",
            "refs/heads/review/*:refs/heads/published/*",
        ],
    );
    let resolver = AssociationResolver::new(Arc::new(PrDemandCoordinator::new(adapter(
        directory.path(),
    ))));
    let capture = resolver
        .capture(&context(&root), Some("review/topic"))
        .await
        .unwrap();
    assert!(!capture.unresolved);
    assert_eq!(
        capture.heads,
        vec![LocalHead {
            repository: RepositoryName {
                owner: "fork".into(),
                name: "project".into()
            },
            head_ref: "published/topic".into()
        }]
    );
}
