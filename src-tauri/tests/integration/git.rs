//! Exercises real Git probe failures and shared operation deadlines.

#[cfg(unix)]
use std::time::Duration;

use super::*;
use crate::test_support::{git, working_tree};
#[cfg(unix)]
use crate::test_support::{deadline_git, executable, ManualClock};

    #[cfg(unix)]
    #[tokio::test]
    async fn a_probe_uses_one_deadline_across_git_operations() {
        let (temp, root) = working_tree();
        let probe = GitProbe::with_executable(&deadline_git(
            temp.path(),
            "rev-parse --absolute-git-dir",
        ));
        let clock = ManualClock::new();
        let probing = tokio::spawn(async move {
            probe.probe_with_deadline(
                &root,
                ProbeDeadline::after(Duration::from_secs(30)),
            ).await
        });
        clock.wait_for_file(&temp.path().join("first-entered")).await;
        clock.advance(Duration::from_secs(20)).await;
        std::fs::write(temp.path().join("first-release"), b"").unwrap();
        // The later operation must spend the first operation's remaining budget, not reset it.
        clock.wait_for_file(&temp.path().join("second-entered")).await;
        clock.advance(Duration::from_secs(11)).await;

        let error = clock.finish(probing).await.unwrap().err();

        assert_eq!(error, Some(GitError::ProbeTimeout));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn head_verification_failure_with_an_existing_target_is_not_unborn() {
        let (temp, root) = working_tree();
        let executable = executable(temp.path(), r#"
if [ "$1 $2 $3" = "rev-parse --verify HEAD" ]; then
    printf 'fatal: simulated HEAD read failure\n' >&2
    exit 128
fi
exec git "$@"
"#);
        let probe = GitProbe::with_executable(&executable);

        assert_eq!(probe.probe(&root).await.err(), Some(GitError::Unavailable));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_reference_inspection_is_not_treated_as_an_absent_branch() {
        let (temp, root) = working_tree();
        git(&root, &["checkout", "--orphan", "unborn"]);
        let executable = executable(temp.path(), r#"
if [ "$1" = show-ref ]; then
    printf 'fatal: cannot read reference storage\n' >&2
    exit 128
fi
exec git "$@"
"#);
        let probe = GitProbe::with_executable(&executable);

        assert_eq!(probe.probe(&root).await.err(), Some(GitError::Unavailable));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_reference_output_is_not_treated_as_an_absent_branch() {
        let (temp, root) = working_tree();
        git(&root, &["checkout", "--orphan", "unborn"]);
        let executable = executable(temp.path(), r#"
if [ "$1" = show-ref ]; then
    printf 'invalid-object-id refs/heads/main\n'
    exit 0
fi
exec git "$@"
"#);
        let probe = GitProbe::with_executable(&executable);

        assert_eq!(probe.probe(&root).await.err(), Some(GitError::Unavailable));
    }

    #[tokio::test]
    async fn absent_symbolic_target_is_unborn_even_when_other_branches_exist() {
        let (_temp, root) = working_tree();
        git(&root, &["checkout", "--orphan", "unborn"]);

        let facts = GitProbe::default().probe(&root).await.unwrap();

        assert_eq!(facts.head, Head::Unborn("unborn".into()));
    }
