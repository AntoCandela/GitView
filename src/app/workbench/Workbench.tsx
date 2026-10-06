/** Composes stable file/ancestry panes around mutually exclusive working, changed or pinned-commit previews. */

import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import type { ReviewSelection } from "../../contracts/diff";
import type { RepositoryFileSelection } from "../../contracts/browsing";
import type { RepositoryClient } from "../../contracts/repositories";
import { FileReview, CommitFileReview, RepositoryFileReview, type CommitReviewSelection, type CommitComparisonControls } from "../../features/diff";
import { ResizableWorkbench } from "./ResizableWorkbench";
import { ChangedFileList, type ObservationView } from "../../features/changes";
import { defaultWorkbenchLayout, type WorkbenchLayoutId } from "./workbenchLayout";
import { useTranslation } from "../../i18n";
import type { AppliedReviewHandoff } from "../companion/useMainReviewSurface";

type ChangeSelection = ReviewSelection;

/** Live selection follows stable native-path identity; committed selection uses native-authorized pinned endpoints. */
export function Workbench({ observation, client, entryId, selectionGeneration, contextLabel, children, onRecheck,
  repositoryFile, onRepositoryFileDismiss, layout = defaultWorkbenchLayout, handoff = null, enabled = true }: {
  observation: ObservationView;
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  contextLabel: string;
  children?: ReactNode | ((comparison: CommitComparisonControls) => ReactNode);
  onRecheck?: () => void;
  layout?: WorkbenchLayoutId;
  repositoryFile?: RepositoryFileSelection | null;
  onRepositoryFileDismiss?: () => void;
  handoff?: AppliedReviewHandoff | null;
  enabled?: boolean;
}) {
  const { t } = useTranslation();
  const scope = useMemo(() => ({ client, entryId, selectionGeneration }), [client, entryId, selectionGeneration]);
  const [chosen, setChosen] = useState<{
    scope: typeof scope;
    handoffId: string | null;
    choice: { kind: "live"; selection: ChangeSelection } | { kind: "commit"; selection: CommitReviewSelection } | null;
  } | null>(null);
  const handoffTarget = handoff?.target;
  const inherited = handoffTarget?.kind === "live" || handoffTarget?.kind === "no_remaining"
    ? { kind: "live" as const, selection: handoffTarget.selection } : null;
  const handoffCurrent = !!handoff && chosen?.handoffId !== handoff.requestId;
  const choice = !repositoryFile ? handoffCurrent ? inherited : chosen?.scope === scope ? chosen.choice : null : null;
  const selection = choice?.kind === "live" ? choice.selection : null;
  const historicalSelection = choice?.kind === "commit" ? choice.selection : null;
  const selectChange = (next: ChangeSelection) => {
    onRepositoryFileDismiss?.();
    setChosen({ scope, handoffId: handoff?.requestId ?? null, choice: { kind: "live", selection: next } });
  };
  const selectCommit = useCallback((next: CommitReviewSelection) => {
    onRepositoryFileDismiss?.();
    setChosen({ scope, handoffId: handoff?.requestId ?? null, choice: { kind: "commit", selection: next } });
  }, [scope, handoff?.requestId, onRepositoryFileDismiss]);
  const previewIntent = useMemo(() => ({ scope, chosen, repositoryFile, handoffId: handoff?.requestId }), [scope, chosen, repositoryFile, handoff?.requestId]);
  const currentPreviewIntent = useRef(previewIntent);
  currentPreviewIntent.current = previewIntent;
  const captureAutoSelection = useCallback(() => {
    const intent = currentPreviewIntent.current;
    return (next: CommitReviewSelection) => {
      if (currentPreviewIntent.current === intent) selectCommit(next);
    };
  }, [selectCommit]);
  const invalidateCommit = useCallback(() => {
    setChosen((current) => current?.choice?.kind === "commit" ? { ...current, choice: null } : current);
  }, []);
  const comparison = useMemo(() => ({
    selection: historicalSelection, onSelect: selectCommit, captureAutoSelection, onInvalidate: invalidateCommit,
  }), [historicalSelection, selectCommit, captureAutoSelection, invalidateCommit]);
  const files = observation.kind === "ready" ? observation.files : null;
  const selectedFile = selection && files?.find((file) => file.stablePathId === selection.stablePathId);
  const categories = selectedFile ? (["unstaged", "staged", "untracked"] as const).filter((category) =>
    category === "untracked" ? selectedFile.untracked : selectedFile[category] !== null,
  ) : selection ? [selection.category] : [];
  if (selection && !categories.includes(selection.category)) categories.unshift(selection.category);
  return (
    <ResizableWorkbench layout={layout} history={typeof children === "function" ? children(comparison) : children}
      comparison={!enabled ? null : handoffCurrent && handoffTarget?.kind === "unavailable" && !repositoryFile ? <div className="comparison-empty" role="status">{t("companion.unavailable")}</div> : repositoryFile ? (
      <RepositoryFileReview client={client} entryId={entryId} selectionGeneration={selectionGeneration}
        contextLabel={contextLabel} selection={repositoryFile} />
    ) : historicalSelection ? (
      <CommitFileReview client={client} entryId={entryId} selectionGeneration={selectionGeneration}
        contextLabel={contextLabel} selection={historicalSelection} />
    ) : selection ? (
      <FileReview client={client} entryId={entryId} selectionGeneration={selectionGeneration} contextLabel={contextLabel}
        observation={observation} selection={selection} categories={categories}
        initialAuthority={handoffCurrent && handoffTarget?.kind === "live" ? {
          entryId: handoffTarget.entryId, stablePathId: handoffTarget.selection.stablePathId, category: handoffTarget.selection.category,
          pathId: handoffTarget.pathId, observationRevision: handoffTarget.observationRevision,
        } : undefined}
        enabled={enabled} outcome={handoffCurrent && handoffTarget?.kind === "no_remaining"
          && (observation.kind !== "ready" || observation.observationRevision <= handoff!.observationRevision) ? "no_remaining" : undefined}
        onCategoryChange={(category) => { if (selection) selectChange({ ...selection, category }); }} />
    ) : <div className="comparison-empty">{t("app.selectFile")}</div>} files={
      <ChangedFileList observation={observation} entryId={entryId} selectionGeneration={selectionGeneration}
        selection={selection} onSelect={selectChange} onRecheck={onRecheck} />
    } />
  );
}
