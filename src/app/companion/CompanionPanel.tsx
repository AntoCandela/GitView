/** Composes the shared workbench UI with admitted-context authority and companion-only lifecycle actions. */
import { useEffect, useRef, useState } from "react";
import type { CompanionClient, ReviewSurfaceClient } from "../../contracts/companion";
import { companionClient, reviewSurfaceClient } from "../../platform/RepositoryClient";
import { ChangedFileList } from "../../features/changes";
import { FileReview } from "../../features/diff";
import { RepositoryBrowser, RepositorySelector, headLabel } from "../../features/repositories";
import { useTranslation } from "../../i18n";
import { CompanionPresentationProvider } from "../CompanionPresentation";
import { Tooltip } from "../../ui/Tooltip";
import { Brand } from "../../ui/Brand";
import { AlertIcon, OpenInAppIcon, QuitIcon } from "../../ui/icons";
import { WorkbenchToolbar } from "../WorkbenchToolbar";
import { ResizableWorkbench } from "../workbench/ResizableWorkbench";
import { useCompanionReview, type CompanionReview } from "./useCompanionReview";
import "./companion.scss";

export function CompanionPanel({ client = companionClient, surfaceClient = reviewSurfaceClient }: {
  client?: CompanionClient; surfaceClient?: ReviewSurfaceClient;
}) {
  const review = useCompanionReview(client, surfaceClient);
  return <CompanionPresentationProvider presentation={review.snapshot?.presentation ?? null}>
    <CompanionContent client={client} review={review} />
  </CompanionPresentationProvider>;
}

function CompanionContent({ client, review }: { client: CompanionClient; review: CompanionReview }) {
  const { t, locale } = useTranslation();
  const panel = useRef<HTMLDivElement>(null);
  const controls = useRef<HTMLDivElement>(null);
  const [repositoriesOpen, setRepositoriesOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const entries = review.snapshot?.workspace.entries ?? [];
  const active = entries.find((entry) => entry.id === review.snapshot?.workspace.activeContextId);
  const query = searchQuery.trim().toLocaleLowerCase();
  const visibleEntries = query ? entries.filter((entry) =>
    entry.repositoryLabel.toLocaleLowerCase().includes(query)
    || entry.locationLabel.toLocaleLowerCase().includes(query)
    || headLabel(entry, locale).toLocaleLowerCase().includes(query)) : entries;
  const selection = review.selection;
  const selectedFile = review.observation?.kind === "ready" && selection ? review.observation.files.find((file) => file.stablePathId === selection.stablePathId) : null;
  const categories = selectedFile ? (["unstaged", "staged", "untracked"] as const).filter((category) =>
    category === "untracked" ? selectedFile.untracked : selectedFile[category] !== null) : selection ? [selection.category] : [];
  if (selection && !categories.includes(selection.category)) categories.unshift(selection.category);
  useEffect(() => {
    if (review.visible) panel.current?.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
    else setRepositoriesOpen(false);
  }, [review.visible]);
  if (!review.visible) return null;
  const errorKey = review.errorSource === "context" ? "companion.contextFailed" : review.errorSource === "surface" ? "companion.unavailable"
    : review.error === "delivery_timeout" ? "companion.handoffTimeout" : review.error === "busy" ? "companion.handoffBusy"
    : review.error === "window_unavailable" ? "companion.handoffUnavailable" : "companion.handoffFailed";
  return <div className="workspace-shell companion-panel" ref={panel} onKeyDown={(event) => {
    if (event.key === "Escape" && !event.defaultPrevented) {
      event.preventDefault();
      void review.lifecycleAction("dismiss");
    }
  }}>
    <WorkbenchToolbar branded={!!active} identity={active ? <Brand compact /> : null}
      selector={<RepositorySelector active={active ?? null} open={repositoriesOpen} onOpenChange={setRepositoriesOpen}
        controlsRef={controls} disabled={!entries.length}>
        <RepositoryBrowser query={searchQuery} onQueryChange={setSearchQuery} list={{
          entries: visibleEntries,
          emptyMessage: t(entries.length ? "app.noMatchingRepositories" : "app.noRepositories"),
          selectedId: review.snapshot?.workspace.activeContextId ?? null,
          actionsDisabled: review.selecting,
          selectionDisabled: review.selecting,
          onSelect: (entryId) => {
            setRepositoriesOpen(false);
            review.selectContext(entryId);
            controls.current?.querySelector<HTMLButtonElement>(".repository-selector")?.focus();
          },
        }} />
      </RepositorySelector>}
      actions={<>
        <Tooltip content={t("companion.openInGitView")} trigger={<button type="button" className="ui-kebab-trigger"
          aria-label={t("companion.openInGitView")} disabled={review.handingOff || review.selecting}
          onClick={() => void review.openInGitView()}><OpenInAppIcon aria-hidden="true" /></button>} />
        <Tooltip content={t("companion.quit")} trigger={<button type="button" className="ui-kebab-trigger"
          aria-label={t("companion.quit")} onClick={() => void review.lifecycleAction("quit")}><QuitIcon aria-hidden="true" /></button>} />
      </>} />
    {review.error ? <div className="error-banner companion-feedback" role="alert"><AlertIcon aria-hidden="true" /><span>{t(errorKey)}</span></div> : null}
    {review.snapshot?.presentation?.persistenceError ? <div className="error-banner persistence-warning companion-feedback" role="alert">
      <AlertIcon aria-hidden="true" /><span>{t("companion.presentationSaveError")}</span>
    </div> : null}
    <main className="workspace-main" aria-label={t("companion.title")}>
      {!review.ready ? <div className="comparison-empty" role="status">{t(review.error ? "companion.unavailable" : "companion.checking")}</div>
        : !active ? <div className="comparison-empty" role="status">{t("companion.noContext")}</div>
        : active.availability === "unavailable" && !review.observation ? <div className="comparison-empty" role="status">{t("companion.unavailable")}</div>
        : <ResizableWorkbench files={
          <ChangedFileList observation={review.observation ?? { kind: active.kind === "bare" ? "bare" : "checking", entryId: active.id, observationRevision: 0 }}
            entryId={active.id} selectionGeneration={review.generation} selection={selection} onSelect={review.selectFile} />
        } comparison={
            selection && review.observation && review.snapshot?.presentation ? <FileReview client={client}
              entryId={active.id} selectionGeneration={review.generation} contextLabel={active.repositoryLabel}
              onVerified={review.recordAuthority}
              observation={review.observation} selection={selection} categories={categories}
              presentation={review.snapshot.presentation} onCategoryChange={(category) => review.selectFile({ ...selection, category })} />
              : <div className="comparison-empty">{t("app.selectFile")}</div>
        } />}
    </main>
  </div>;
}
