/** Presents observed changed files and availability while the app owns review selection. */
import { useMemo } from "react";
import type { ObservationErrorCode } from "../../contracts/changes";
import type { ReviewCategory, ReviewSelection } from "../../contracts/diff";
import { FileExplorer } from "../../ui/file-explorer/FileExplorer";
import { Tooltip } from "../../ui/Tooltip";
import { changedPathTreeFile } from "./changedPathTreeFile";
import type { ObservationView } from "./useObservation";

const errorLabels: Record<ObservationErrorCode, string> = {
  inaccessible: "The repository location cannot be accessed.",
  git_unavailable: "Git is unavailable.",
  unsafe_repository: "Git requires trusted repository ownership.",
  timeout: "The repository check timed out.",
  invalid_status: "Git returned status that could not be read safely.",
  unsupported_path_encoding: "A native path cannot be displayed safely.",
  resource_limit: "The repository status exceeds the observation limit.",
  unsupported_configuration: "This repository configuration is not supported for safe read-only inspection.",
};

export function ChangedFileList({ observation, entryId, selectionGeneration, selection, onSelect, onRecheck }: {
  observation: ObservationView;
  entryId: string;
  selectionGeneration: number;
  selection: ReviewSelection | null;
  onSelect: (selection: ReviewSelection) => void;
  onRecheck?: () => void;
}) {
  const files = observation.kind === "ready" ? observation.files : null;
  const treeFiles = useMemo(() => (files ?? []).map(changedPathTreeFile), [files]);
  let status: { title: string; description: string } | null = null;
  if (observation.kind !== "ready") {
    const title = observation.kind === "bare" ? "No working tree"
      : observation.kind === "checking" ? "Checking changes…" : "Changes unavailable";
    const description = observation.kind === "bare" ? "Bare repositories have no working-tree files."
      : observation.kind === "checking" ? "Waiting for the repository observation."
      : observation.kind === "transport_unavailable" ? "Desktop connection interrupted. Reconnecting automatically."
      : `${errorLabels[observation.errorCode]} Checking again automatically.`;
    status = { title, description };
  } else if (files?.length === 0) {
    status = { title: "Clean", description: "No changed files in this working tree." };
  }
  return <>
    <FileExplorer key={`${entryId}:${selectionGeneration}`} files={treeFiles}
      count={files?.length ?? null} treeLabel="Changed file hierarchy" listLabel="Changed file list"
      selectedId={selection?.stablePathId} onSelect={(item) => {
        const file = files?.find((candidate) => candidate.stablePathId === item.id);
        if (!file) return;
        const category: ReviewCategory = file.unstaged !== null ? "unstaged" : file.staged !== null ? "staged" : file.untracked ? "untracked" : "unstaged";
        onSelect({ stablePathId: file.stablePathId, category, displayPath: file.displayPath });
      }}>
      {status ? <div className="changes-state" role="status"><h3>{status.title}</h3>{observation.kind === "unavailable" || observation.kind === "transport_unavailable" ? <p>{status.description}</p> : null}</div> : null}
    </FileExplorer>
    {onRecheck ? <Tooltip content="Check repository changes again"
      trigger={<button type="button" className="retry-button" onClick={onRecheck}>Check again</button>} /> : null}
  </>;
}
