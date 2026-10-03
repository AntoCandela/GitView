/** Composes stable file/ancestry panes around mutually exclusive working, changed or pinned-commit previews. */

import { useCallback, useMemo, useState, type ReactNode } from "react";
import type { ReviewSelection } from "../../contracts/diff";
import type { RepositoryFileSelection } from "../../contracts/browsing";
import type { RepositoryClient } from "../../contracts/repositories";
import { FileReview, CommitFileReview, RepositoryFileReview, type CommitReviewSelection, type CommitComparisonControls } from "../../features/diff";
import { ResizableWorkbench } from "./ResizableWorkbench";
import { ChangedFileList, type ObservationView } from "../../features/changes";
import { defaultWorkbenchLayout, type WorkbenchLayoutId } from "./workbenchLayout";

type ChangeSelection = ReviewSelection;

/** Live selection follows stable native-path identity; committed selection uses native-authorized pinned endpoints. */
export function Workbench({ observation, client, entryId, selectionGeneration, contextLabel, children, onRecheck,
  repositoryFile, onRepositoryFileDismiss, layout = defaultWorkbenchLayout }: {
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
}) {
  const scope = useMemo(() => ({ client, entryId, selectionGeneration }), [client, entryId, selectionGeneration]);
  const [chosen, setChosen] = useState<{
    scope: typeof scope;
    choice: { kind: "live"; selection: ChangeSelection } | { kind: "commit"; selection: CommitReviewSelection };
  } | null>(null);
  const choice = !repositoryFile && chosen?.scope === scope ? chosen.choice : null;
  const selection = choice?.kind === "live" ? choice.selection : null;
  const historicalSelection = choice?.kind === "commit" ? choice.selection : null;
  const selectChange = (next: ChangeSelection) => {
    onRepositoryFileDismiss?.();
    setChosen({ scope, choice: { kind: "live", selection: next } });
  };
  const selectCommit = useCallback((next: CommitReviewSelection) => {
    onRepositoryFileDismiss?.();
    setChosen({ scope, choice: { kind: "commit", selection: next } });
  }, [scope, onRepositoryFileDismiss]);
  const invalidateCommit = useCallback(() => {
    setChosen((current) => current?.choice.kind === "commit" ? null : current);
  }, []);
  const comparison = useMemo(() => ({
    selection: historicalSelection, onSelect: selectCommit, onInvalidate: invalidateCommit,
  }), [historicalSelection, selectCommit, invalidateCommit]);
  const files = observation.kind === "ready" ? observation.files : null;
  const selectedFile = selection && files?.find((file) => file.stablePathId === selection.stablePathId);
  const categories = selectedFile ? (["unstaged", "staged", "untracked"] as const).filter((category) =>
    category === "untracked" ? selectedFile.untracked : selectedFile[category] !== null,
  ) : selection ? [selection.category] : [];
  return (
    <ResizableWorkbench layout={layout} history={typeof children === "function" ? children(comparison) : children}
      comparison={repositoryFile ? (
      <RepositoryFileReview client={client} entryId={entryId} selectionGeneration={selectionGeneration}
        contextLabel={contextLabel} selection={repositoryFile} />
    ) : historicalSelection ? (
      <CommitFileReview client={client} entryId={entryId} selectionGeneration={selectionGeneration}
        contextLabel={contextLabel} selection={historicalSelection} />
    ) : selection ? (
      <FileReview client={client} entryId={entryId} selectionGeneration={selectionGeneration} contextLabel={contextLabel}
        observation={observation} selection={selection} categories={categories}
        onCategoryChange={(category) => { if (selection) selectChange({ ...selection, category }); }} />
    ) : <div className="comparison-empty">Select a changed file to compare.</div>} files={
      <ChangedFileList observation={observation} entryId={entryId} selectionGeneration={selectionGeneration}
        selection={selection} onSelect={selectChange} onRecheck={onRecheck} />
    } />
  );
}
