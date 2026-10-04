/**
 * Composes centered context selectors, the modular sidebar and full-bleed file/ancestry workbench.
 * Confirmed context remains native-owned; browsing selections and disclosure are local.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RepositoryClient } from "../contracts/repositories";
import type { ReviewSurfaceClient, ReviewHandoffClient } from "../contracts/companion";
import type { RepositoryFileSelection } from "../contracts/browsing";
import { repositoryClient, reviewSurfaceClient, reviewHandoffClient } from "../platform/RepositoryClient";
import { useObservation } from "../features/changes";
import { Workbench } from "./workbench/Workbench";
import { WorkbenchToolbar } from "./WorkbenchToolbar";
import { WorkbenchLayoutMenu } from "./workbench/WorkbenchLayoutMenu";
import { defaultWorkbenchLayout, type WorkbenchLayoutId } from "./workbench/workbenchLayout";
import { HistoryGraph } from "../features/history";
import { AppearanceMenu, useAppearanceTheme } from "../features/appearance";
import { Tooltip } from "../ui/Tooltip";
import { IconThemeProvider } from "../ui/file-icons/IconThemeProvider";
import { Brand } from "../ui/Brand";
import { resetPanelLayout } from "../ui/resize/panelLayout";

import {
  AlertIcon,
  CloseIcon,
  SidebarIcon,
  RefreshIcon,
  ResetLayoutIcon,
} from "../ui/icons";
import { RepositoryBrowser, RepositorySelector, RepositoryFiles, headLabel, useWorkspace, type RepositoryRenameState } from "../features/repositories";
import { WorkspaceSidebar } from "./WorkspaceSidebar";
import { useTranslation, useLocale } from "../i18n";
import { workspaceErrorMessage } from "../features/repositories";
import { LanguageMenu } from "./LanguageMenu";
import { CompanionSettings } from "./CompanionSettings";
import { CompanionPresentationPublisher } from "./CompanionPresentation";
import { useMainReviewSurface, type AppliedReviewHandoff } from "./companion/useMainReviewSurface";
import { useReviewSurface } from "./companion/useReviewSurface";
import "./companion/companion.scss";

export function Workspace({
  client = repositoryClient,
  surfaceClient = client === repositoryClient ? reviewSurfaceClient : null,
  handoffClient = reviewHandoffClient,
}: {
  client?: RepositoryClient;
  surfaceClient?: ReviewSurfaceClient | null;
  handoffClient?: ReviewHandoffClient;
}) {
  const { locale, t } = useTranslation();
  const localeState = useLocale();
  const appearance = useAppearanceTheme();
  const surface = useReviewSurface(surfaceClient);
  const surfaceVisible = surfaceClient === null || (surface.snapshot?.visible ?? false);
  useLayoutEffect(() => {
    document.documentElement.dataset.appearance = appearance.theme;
    return () => { if (document.documentElement.dataset.appearance === appearance.theme) delete document.documentElement.dataset.appearance; };
  }, [appearance.theme]);
  const {
    snapshot,
    acceptExternalSnapshot,
    loading,
    opening,
    mutating,
    pendingId,
    selectionGeneration,
    error,
    openRepository,
    selectRepository,
    selectWorktree,
    renameRepository,
    removeRepository,
    refreshAvailability,
    dismissError,
  } = useWorkspace(client, surfaceVisible);
  const [searchQuery, setSearchQuery] = useState("");
  const [layout, setLayout] = useState<WorkbenchLayoutId>(defaultWorkbenchLayout);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [browsedFile, setBrowsedFile] = useState<{ entryId: string; generation: number; file: RepositoryFileSelection } | null>(null);
  const dismissBrowsedFile = useCallback(() => setBrowsedFile(null), []);
  const [handoff, setHandoff] = useState<AppliedReviewHandoff | null>(null);
  useMainReviewSurface(surface.connection, handoffClient, acceptExternalSnapshot, (incoming) => {
    setBrowsedFile(null);
    setHandoff(incoming);
  });
  const shellRef = useRef<HTMLDivElement>(null);
  const controlsRef = useRef<HTMLDivElement>(null);
  const [repositoriesOpen, setRepositoriesOpen] = useState(false);
  const [renameState, setRenameState] = useState<RepositoryRenameState | null>(null);
  const renameStateRef = useRef(renameState);
  renameStateRef.current = renameState;
  const renameSubmitting = useRef(false);

  function restoreRenameFocus(entryId: string) {
    requestAnimationFrame(() => {
      const rows = shellRef.current?.querySelectorAll<HTMLElement>("[data-repository-id]");
      const target = Array.from(rows ?? []).find((row) => row.dataset.repositoryId === entryId);
      const focusTarget = target?.querySelector<HTMLElement>('button[aria-haspopup="menu"]:not(:disabled)') ??
        target?.querySelector<HTMLElement>("[data-repository-index]") ??
        shellRef.current?.querySelector<HTMLElement>('[aria-current="true"]') ??
        shellRef.current?.querySelector<HTMLElement>('input[type="search"]');
      focusTarget?.focus();
    });
  }

  function cancelRename(restoreFocus = true) {
    const entryId = renameStateRef.current?.entryId;
    renameStateRef.current = null;
    setRenameState(null);
    if (restoreFocus && entryId) restoreRenameFocus(entryId);
  }

  async function saveRename() {
    const draft = renameStateRef.current;
    if (!draft || !draft.displayName.trim() || renameSubmitting.current) return;
    renameSubmitting.current = true;
    setRenameState({ ...draft, busy: true, error: null });
    try {
      const message = await renameRepository(draft.entryId, draft.displayName.trim());
      // Dismissing/filtering the editor does not cancel a native intent already sent.
      if (renameStateRef.current?.entryId !== draft.entryId) return;
      if (message === null) cancelRename();
      else setRenameState((current) => current && { ...current, busy: false, error: message });
    } finally {
      renameSubmitting.current = false;
    }
  }

  // Hide prior details until the requested selection is confirmed, not under its new highlight.
  const active =
    pendingId === null
      ? snapshot.entries.find((entry) => entry.id === snapshot.activeContextId)
      : undefined;
  const observed = useObservation(client, active?.id ?? null, selectionGeneration, surfaceVisible && surfaceClient === null);
  const sharedObservation = surfaceVisible && !surface.reconciling && surface.snapshot && active
    && surface.snapshot.workspace.contextEpoch === snapshot.contextEpoch && surface.snapshot.observation?.entryId === active.id
    ? surface.snapshot.observation : null;
  const observation = (surfaceClient ? sharedObservation : observed)
    ?? (active ? { kind: "checking" as const, entryId: active.id, observationRevision: 0 } : null);
  const repositoryFile = browsedFile?.entryId === active?.id && browsedFile?.generation === selectionGeneration ? browsedFile.file : null;
  const workingBranch = active?.head.kind === "branch" || active?.head.kind === "unborn" ? active.head.name : null;
  const query = searchQuery.trim().toLocaleLowerCase();
  const visibleEntries = query
    ? snapshot.entries.filter(
        (entry) =>
          entry.repositoryLabel.toLocaleLowerCase().includes(query) ||
          headLabel(entry, locale).toLocaleLowerCase().includes(query) ||
          entry.locationLabel.toLocaleLowerCase().includes(query),
      )
    : snapshot.entries;
  useEffect(() => {
    if (renameState && !visibleEntries.some((entry) => entry.id === renameState.entryId))
      cancelRename(document.activeElement === document.body);
  }, [visibleEntries, renameState]);

  const repositoryBrowser = <RepositoryBrowser query={searchQuery} onQueryChange={setSearchQuery} opening={opening}
      onOpen={() => void openRepository()} list={{
        entries: visibleEntries,
        emptyMessage: t(snapshot.entries.length === 0 ? "app.noRepositories" : "app.noMatchingRepositories"),
        selectedId: pendingId ?? snapshot.activeContextId,
        onSelect: (entryId) => {
          setRepositoriesOpen(false);
          selectRepository(entryId);
          controlsRef.current?.querySelector<HTMLButtonElement>(".repository-selector")?.focus();
        },
        actionsDisabled: loading || mutating,
        management: {
          onRename: (entry) => {
            const draft = { entryId: entry.id, displayName: entry.repositoryLabel, busy: false, error: null };
            renameStateRef.current = draft;
            setRenameState(draft);
          },
          onRemove: (entryId) => void removeRepository(entryId),
          renameState,
          onRenameChange: (displayName) => setRenameState((current) => current && { ...current, displayName, error: null }),
          onRenameSave: () => void saveRename(),
          onRenameCancel: cancelRename,
        },
      }} />;
  return (
    <IconThemeProvider><div className="workspace-shell" ref={shellRef}>
      {client === repositoryClient ? <CompanionPresentationPublisher /> : null}
      <WorkbenchToolbar branded={Boolean(active)} identity={<>
        {active ? <Brand compact /> : null}
        <Tooltip content={t(sidebarOpen ? "app.collapseSidebar" : "app.expandSidebar")} trigger={
          <button type="button" className="workspace-sidebar-toggle" aria-label={t(sidebarOpen ? "app.collapseSidebar" : "app.expandSidebar")}
            aria-expanded={sidebarOpen} aria-controls="workspace-sidebar"
            onClick={() => { setSidebarOpen((current) => !current); setRepositoriesOpen(false); }}>
            <SidebarIcon size={16} aria-hidden="true" />
          </button>
        } />
        </>} selector={
          <RepositorySelector active={active ?? null} open={repositoriesOpen}
            onOpenChange={setRepositoriesOpen} controlsRef={controlsRef}>
            {repositoryBrowser}
          </RepositorySelector>
        } actions={<>
          <Tooltip content={t("app.resetLayout")} trigger={
            <button type="button" className="ui-kebab-trigger" aria-label={t("app.resetLayout")}
              onClick={() => {
                resetPanelLayout();
                setLayout(defaultWorkbenchLayout);
                setSidebarOpen(true);
              }}>
              <ResetLayoutIcon aria-hidden="true" />
            </button>
          } />
          <AppearanceMenu />
          <WorkbenchLayoutMenu value={layout} onChange={setLayout} />
          <LanguageMenu />
          {client === repositoryClient ? <CompanionSettings refreshRevision={surface.noticeRevision} /> : null}
        </>} />
      <div className="workspace-body">
        <WorkspaceSidebar open={sidebarOpen}>
          {active && active.kind !== "unknown" ? <RepositoryFiles key={`${active.id}:${selectionGeneration}`}
            client={client} entryId={active.id} enabled={sidebarOpen && surfaceVisible} observation={observation} selected={repositoryFile}
            onSelect={(file) => setBrowsedFile({ entryId: active.id, generation: selectionGeneration, file })} />
            : <p className="empty-list">{t(pendingId ? "app.switchingRepository" : "app.chooseFilesRepository")}</p>}
        </WorkspaceSidebar>

      <main className="workspace-main">
        {localeState.persistenceError ? <div className="error-banner persistence-warning" role="alert">
          <AlertIcon aria-hidden="true" /><span>{t("common.sessionOnly")}</span>
        </div> : null}
        {snapshot.persistenceError ? (
          <div className="error-banner persistence-warning" role="alert">
            <AlertIcon aria-hidden="true" />
            <span>
              {t(`app.persistence.${snapshot.persistenceError.code}`)}
            </span>
          </div>
        ) : null}
        {error ? (
          <div className="error-banner" role="alert">
            <AlertIcon aria-hidden="true" />
            <span>{workspaceErrorMessage(error, locale)}</span>
            <Tooltip content={t("app.dismissError")} trigger={<button
              type="button"
              onClick={dismissError}
              aria-label={t("app.dismissError")}
            >
              <CloseIcon aria-hidden="true" />
            </button>} />
          </div>
        ) : null}
        {snapshot.restoring ? (
          <p className="workspace-restoring" role="status">
            {t("app.checkingSaved")}
          </p>
        ) : null}
        {pendingId !== null ? (
          <section className="workspace-state" role="status"><h2>{t("app.switchingRepository")}</h2></section>
        ) : active ? (
          active.kind === "unknown" ? <section className="workspace-state" role="status">
            <p>{t(active.availability === "unavailable" ? "app.repositoryUnavailable" : "app.checkingRepository")}</p>
            {active.availability === "unavailable" ? <Tooltip content={t("app.checkAgainHint")} trigger={
              <button className="retry-button" type="button" onClick={() => refreshAvailability(active.id)}>
                <RefreshIcon aria-hidden="true" />{t("app.checkAgain")}
              </button>} /> : null}
          </section> : observation ? <Workbench
            key={`${active.id}:${selectionGeneration}`}
            observation={observation} client={client} entryId={active.id}
            selectionGeneration={selectionGeneration} contextLabel={active.repositoryLabel}
            layout={layout}
            handoff={handoff?.contextEpoch === snapshot.contextEpoch ? handoff : null}
            enabled={surfaceVisible}
            repositoryFile={repositoryFile} onRepositoryFileDismiss={dismissBrowsedFile}
            onRecheck={active.availability === "unavailable" ? () => refreshAvailability(active.id) : undefined}>
            {(comparison) => surfaceVisible ? <HistoryGraph client={client} entryId={active.id} selectionGeneration={selectionGeneration}
              comparison={comparison}
              workingBranch={workingBranch}
              onSelectWorktree={(worktreeId) => selectWorktree(active.id, worktreeId)} /> : null}
          </Workbench> : null
        ) : (
          <section className="workspace-state workspace-state--branded" role="status">
            <Brand />
            <h2>{t(snapshot.entries.length ? "app.chooseRepository" : "app.noRepository")}</h2>
            <p className="brand-description">{t("app.tagline")}<br />{t("app.description")}</p>
            {loading ? <p>{t("app.connecting")}</p> : null}
          </section>
        )}
      </main>
      </div>
    </div></IconThemeProvider>
  );
}
