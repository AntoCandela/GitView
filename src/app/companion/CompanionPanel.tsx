/** Composes admitted-context changes and one read-only comparison, never the main workbench. */
import { useEffect, useRef } from "react";
import type { CompanionClient, ReviewSurfaceClient } from "../../contracts/companion";
import { companionClient, reviewSurfaceClient } from "../../platform/RepositoryClient";
import { ChangedFileList } from "../../features/changes";
import { FileReview } from "../../features/diff";
import { useTranslation } from "../../i18n";
import { CompanionPresentationProvider } from "../CompanionPresentation";
import { Tooltip } from "../../ui/Tooltip";
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
  const { t } = useTranslation();
  const panel = useRef<HTMLDivElement>(null);
  const active = review.snapshot?.workspace.entries.find((entry) => entry.id === review.snapshot?.workspace.activeContextId);
  const selection = review.selection;
  const selectedFile = review.observation?.kind === "ready" && selection ? review.observation.files.find((file) => file.stablePathId === selection.stablePathId) : null;
  const categories = selectedFile ? (["unstaged", "staged", "untracked"] as const).filter((category) =>
    category === "untracked" ? selectedFile.untracked : selectedFile[category] !== null) : selection ? [selection.category] : [];
  if (selection && !categories.includes(selection.category)) categories.unshift(selection.category);
  useEffect(() => {
    if (review.visible) panel.current?.querySelector<HTMLElement>("select:not(:disabled),button:not(:disabled)")?.focus();
  }, [review.visible]);
  if (!review.visible) return null;
  const errorKey = review.errorSource === "context" ? "companion.contextFailed" : review.errorSource === "surface" ? "companion.unavailable"
    : review.error === "delivery_timeout" ? "companion.handoffTimeout" : review.error === "busy" ? "companion.handoffBusy"
    : review.error === "window_unavailable" ? "companion.handoffUnavailable" : "companion.handoffFailed";
  return <div className="companion-panel" ref={panel} onKeyDown={(event) => {
    if (event.key === "Escape") { event.preventDefault(); void review.lifecycleAction("dismiss"); }
  }}>
    <header className="companion-header">
      <label htmlFor="companion-context">{t("companion.context")}</label>
      <select id="companion-context" value={review.snapshot?.workspace.activeContextId ?? ""}
        disabled={!review.snapshot?.workspace.entries.length || review.selecting}
        onChange={(event) => review.selectContext(event.target.value)}>
        {!review.snapshot?.workspace.activeContextId ? <option value="">{t("companion.noContext")}</option> : null}
        {review.snapshot?.workspace.entries.map((entry) => <option key={entry.id} value={entry.id}>
          {entry.repositoryLabel} — {entry.locationLabel}
        </option>)}
      </select>
    </header>
    {review.error ? <div className="companion-feedback" role="alert">{t(errorKey)}</div> : null}
    {review.snapshot?.presentation?.persistenceError ? <div className="companion-feedback" role="alert">{t("companion.presentationSaveError")}</div> : null}
    <main className="companion-review" aria-label={t("companion.title")}>
      {!review.ready ? <div className="companion-state" role="status">{t(review.error ? "companion.unavailable" : "companion.checking")}</div>
        : !active ? <div className="companion-state" role="status">{t("companion.noContext")}</div>
        : active.availability === "unavailable" && !review.observation ? <div className="companion-state" role="status">{t("companion.unavailable")}</div>
        : <>
          <section className="companion-files">
            <ChangedFileList observation={review.observation ?? { kind: active.kind === "bare" ? "bare" : "checking", entryId: active.id, observationRevision: 0 }}
              entryId={active.id} selectionGeneration={review.generation} selection={selection} onSelect={review.selectFile} />
          </section>
          <div className="companion-diff" tabIndex={0} aria-label={t("diff.selectedReview")}>
            {selection && review.observation && review.snapshot?.presentation ? <FileReview client={client}
              entryId={active.id} selectionGeneration={review.generation} contextLabel={active.repositoryLabel}
              onVerified={review.recordAuthority}
              observation={review.observation} selection={selection} categories={categories}
              presentation={review.snapshot.presentation} onCategoryChange={(category) => review.selectFile({ ...selection, category })} />
              : <div className="comparison-empty">{t("app.selectFile")}</div>}
          </div>
        </>}
    </main>
    <footer className="companion-footer">
      <Tooltip content={t("companion.openInGitView")} trigger={<button type="button" disabled={review.handingOff || review.selecting}
        onClick={() => void review.openInGitView()}>{t("companion.openInGitView")}</button>} />
      <Tooltip content={t("companion.quit")} trigger={<button type="button" onClick={() => void review.lifecycleAction("quit")}>{t("companion.quit")}</button>} />
    </footer>
  </div>;
}
