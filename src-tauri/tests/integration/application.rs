//! Exercises real repository admission and refresh ordering.

    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::test_support::{
        allow_new_probes, block, deadline_git, executable, gated_git, git, release,
        wait_for_probe, wait_for_reaped_child, working_tree, ManualClock,
    };
    use crate::workspace::{Availability, HeadLabel};
    use crate::diagnostics::{Component, DiagnosticStore, Event, Query, ReadOnlyDiagnostics};

    fn service(executable: &Path) -> Arc<RepositoryService> {
        Arc::new(RepositoryService {
            probe: GitProbe::with_executable(executable),
            workspace: Arc::new(WorkspaceStore::default()),
            ..RepositoryService::default()
        })
    }

    fn opened(outcome: OpenOutcome) -> (String, WorkspaceSnapshot) {
        match outcome {
            OpenOutcome::Opened { entry_id, snapshot } => (entry_id, snapshot),
            other => panic!("repository was not admitted: {other:?}"),
        }
    }

    fn start_refresh(service: &Arc<RepositoryService>, id: &str) -> tokio::task::JoinHandle<WorkspaceSnapshot> {
        let service = service.clone();
        let id = id.to_owned();
        tokio::spawn(async move { service.refresh(&id).await })
    }

    #[tokio::test]
    async fn opening_keeps_snapshots_and_unrelated_selection_responsive() {
        let (temp, root) = working_tree();
        let other = temp.path().join("other");
        fs::create_dir(&other).unwrap();
        git(&other, &["init", "-b", "other"]);
        let service = service(&gated_git(temp.path()));
        let (other_id, before) = opened(service.open_chosen(&other).await);
        block(&root);
        let opening = {
            let service = service.clone();
            let root = root.clone();
            tokio::spawn(async move { service.open_chosen(&root).await })
        };
        wait_for_probe(&root).await;

        let pending = tokio::time::timeout(Duration::from_secs(1), service.snapshot()).await.unwrap();
        assert_eq!(pending, before);
        let selected = tokio::time::timeout(Duration::from_secs(1), service.select(&other_id)).await.unwrap();
        let SelectOutcome::Selected { snapshot: selected } = selected else {
            panic!("unrelated admitted context could not be selected");
        };
        assert_eq!(selected.active_context_id, Some(other_id.clone()));
        assert_eq!(selected.entries[0].availability, Availability::Checking);
        assert!(selected.revision > before.revision);

        allow_new_probes(&root);
        release(&root);
        let (_, admitted) = opened(opening.await.unwrap());
        assert_eq!(admitted.active_context_id, Some(other_id));
        assert_eq!(admitted.entries.len(), 2);
        assert!(admitted.revision > selected.revision);
    }

    #[tokio::test]
    async fn refreshing_keeps_snapshots_and_unrelated_selection_responsive() {
        let (temp, root) = working_tree();
        let other = temp.path().join("other");
        fs::create_dir(&other).unwrap();
        git(&other, &["init", "-b", "other"]);
        let service = service(&gated_git(temp.path()));
        let (id, _) = opened(service.open_chosen(&root).await);
        let (other_id, before) = opened(service.open_chosen(&other).await);
        block(&root);
        let refreshing = start_refresh(&service, &id);
        wait_for_probe(&root).await;

        let pending = tokio::time::timeout(Duration::from_secs(1), service.snapshot()).await.unwrap();
        assert_eq!(pending.entries[0].availability, Availability::Checking);
        assert_eq!(pending.entries[1].availability, Availability::Available);
        assert!(pending.revision > before.revision);
        let selected = tokio::time::timeout(Duration::from_secs(1), service.select(&other_id)).await.unwrap();
        let SelectOutcome::Selected { snapshot: selected } = selected else {
            panic!("unrelated context could not be selected");
        };
        assert_eq!(selected.active_context_id, Some(other_id.clone()));

        allow_new_probes(&root);
        release(&root);
        let completed = refreshing.await.unwrap();
        assert_eq!(completed.active_context_id, Some(other_id));
        assert_eq!(completed.entries[0].availability, Availability::Available);
        assert_eq!(completed.entries[1].availability, Availability::Checking);
        assert!(completed.revision > selected.revision);
    }

    #[tokio::test]
    async fn newer_refresh_completion_ignores_a_stale_head_probe() {
        let (temp, root) = working_tree();
        let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
        let store = DiagnosticStore::open(&database);
        let service = Arc::new(RepositoryService::with_probe_and_diagnostics(GitProbe::with_executable(&gated_git(temp.path())), store.sink()));
        let (id, _) = opened(service.open_chosen(&root).await);
        block(&root);
        let stale = start_refresh(&service, &id);
        wait_for_probe(&root).await;
        git(&root, &["branch", "-m", "updated"]);
        allow_new_probes(&root);

        let current = service.refresh(&id).await;
        assert_eq!(current.entries[0].head, HeadLabel::Branch { name: "updated".into() });
        assert_eq!(current.entries[0].availability, Availability::Available);
        release(&root);
        assert_eq!(stale.await.unwrap(), current);
        assert_eq!(service.snapshot().await, current);
        service.shutdown().await;
        store.flush(Duration::from_secs(2)).unwrap();
        let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { component: Some(Component::Application), limit: 200, ..Default::default() }).unwrap().events;
        let superseded = rows.iter().find(|row| row.event == Event::Superseded).expect("stale refresh must be labeled superseded");
        assert!(!rows.iter().any(|row| row.operation_id == superseded.operation_id && row.event == Event::Completed));
        store.shutdown(Duration::from_secs(2)).unwrap();
    }

    #[tokio::test]
    async fn reused_admission_invalidates_an_older_refresh() {
        let (temp, root) = working_tree();
        let service = service(&gated_git(temp.path()));
        let (id, _) = opened(service.open_chosen(&root).await);
        block(&root);
        let stale = start_refresh(&service, &id);
        wait_for_probe(&root).await;
        git(&root, &["branch", "-m", "updated"]);
        allow_new_probes(&root);

        let OpenOutcome::Reused { entry_id, snapshot: current } = service.open_chosen(&root).await else {
            panic!("existing identity was not reused");
        };
        assert_eq!(entry_id, id);
        assert_eq!(current.entries[0].head, HeadLabel::Branch { name: "updated".into() });
        assert_eq!(current.entries[0].availability, Availability::Available);
        assert_eq!(current.active_context_id, None);
        release(&root);
        assert_eq!(stale.await.unwrap(), current);
    }

    #[tokio::test]
    async fn selecting_an_entry_invalidates_its_pending_refresh() {
        let (temp, root) = working_tree();
        let service = service(&gated_git(temp.path()));
        let (id, _) = opened(service.open_chosen(&root).await);
        block(&root);
        let stale = start_refresh(&service, &id);
        wait_for_probe(&root).await;

        let SelectOutcome::Selected { snapshot: selected } = service.select(&id).await else {
            panic!("context could not be selected");
        };
        assert_eq!(selected.active_context_id, Some(id));
        assert_eq!(selected.entries[0].availability, Availability::Checking);
        allow_new_probes(&root);
        release(&root);
        assert_eq!(stale.await.unwrap(), selected);
    }

    #[tokio::test]
    async fn older_reopen_completion_does_not_overwrite_a_newer_refresh() {
        let (temp, root) = working_tree();
        // Hold the verification pass after reading HEAD, so a later refresh can win admission.
        let executable = executable(temp.path(), r#"
if [ "$1 $2 $3" = "symbolic-ref --quiet --short" ] && [ -f .gitview-block ]; then
    if [ -f .gitview-first-symbolic ]; then
        git "$@" || exit $?
        : > .gitview-entered
        while [ ! -f .gitview-release ]; do sleep 0.01; done
        exit 0
    fi
    : > .gitview-first-symbolic
fi
exec git "$@"
"#);
        let service = service(&executable);
        let (id, _) = opened(service.open_chosen(&root).await);
        block(&root);
        let stale = {
            let service = service.clone();
            let root = root.clone();
            tokio::spawn(async move { service.open_chosen(&root).await })
        };
        wait_for_probe(&root).await;
        git(&root, &["branch", "-m", "updated"]);
        allow_new_probes(&root);

        let current = service.refresh(&id).await;
        assert_eq!(current.entries[0].head, HeadLabel::Branch { name: "updated".into() });
        release(&root);
        let OpenOutcome::Reused { entry_id, snapshot } = stale.await.unwrap() else {
            panic!("older reopen failed to reuse the existing identity");
        };
        assert_eq!(entry_id, id);
        assert_eq!(snapshot, current);
    }

    #[tokio::test]
    async fn cancelled_refresh_reaps_child_and_preserves_a_coherent_checking_snapshot() {
        let (temp, root) = working_tree();
        let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
        let store = DiagnosticStore::open(&database);
        let service = Arc::new(RepositoryService::with_probe_and_diagnostics(GitProbe::with_executable(&gated_git(temp.path())), store.sink()));
        let (id, _) = opened(service.open_chosen(&root).await);
        service.select(&id).await;
        block(&root);
        let refreshing = start_refresh(&service, &id);
        wait_for_probe(&root).await;
        let pending = service.snapshot().await;

        refreshing.abort();
        assert!(refreshing.await.unwrap_err().is_cancelled());
        wait_for_reaped_child(&root).await;
        assert_eq!(service.snapshot().await, pending);
        assert_eq!(pending.active_context_id.as_deref(), Some(id.as_str()));
        assert_eq!(pending.entries[0].availability, Availability::Checking);
        allow_new_probes(&root);
        let recovered = service.refresh(&id).await;
        assert_eq!(recovered.entries[0].availability, Availability::Available);
        assert_eq!(recovered.active_context_id, Some(id));
        assert!(recovered.revision > pending.revision);
        service.shutdown().await;
        store.flush(Duration::from_secs(2)).unwrap();
        let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { limit: 200, ..Default::default() }).unwrap().events;
        let cancelled = rows.iter().find(|row| row.component == Component::Application && row.operation_kind == OperationKind::RefreshAvailability && row.event == Event::Cancelled).expect("dropped refresh must be labeled cancelled");
        assert!(!rows.iter().any(|row| row.component == Component::Application && row.operation_id == cancelled.operation_id && row.event == Event::Completed));
        assert!(rows.iter().any(|row| row.component == Component::Process && row.operation_id == cancelled.operation_id && row.event == Event::CleanupCompleted && !row.cleanup_failed));
        store.shutdown(Duration::from_secs(2)).unwrap();
    }

    #[tokio::test]
    async fn opening_verification_shares_one_deadline() {
        let (temp, root) = working_tree();
        let service = service(&deadline_git(temp.path(), "--version"));
        let clock = ManualClock::new();
        let opening = tokio::spawn(async move {
            service.open_with_deadline(
                &root,
                ProbeDeadline::after(Duration::from_secs(30)),
            ).await
        });
        clock.wait_for_file(&temp.path().join("first-entered")).await;
        clock.advance(Duration::from_secs(20)).await;
        fs::write(temp.path().join("first-release"), b"").unwrap();
        // The second version gate proves that the complete first probe succeeded.
        clock.wait_for_file(&temp.path().join("second-entered")).await;
        clock.advance(Duration::from_secs(11)).await;

        let outcome = clock.finish(opening).await.unwrap();
        let OpenOutcome::Rejected { code, snapshot, .. } = outcome else {
            panic!("double verification escaped its operation deadline");
        };
        assert_eq!(code, GitError::ProbeTimeout);
        assert!(snapshot.entries.is_empty());
        assert_eq!(snapshot.revision, 0);
        assert_eq!(snapshot.active_context_id, None);
    }

#[path = "application_persistence.rs"]
mod persistence;
