/** Presents observed changed files and availability while the app owns review selection. */
import { useMemo } from "react";
import type { ObservationErrorCode } from "../../contracts/changes";
import type { ReviewCategory, ReviewSelection } from "../../contracts/diff";
import { FileExplorer } from "../../ui/file-explorer/FileExplorer";
import { Tooltip } from "../../ui/Tooltip";
import { changedPathTreeFile } from "./changedPathTreeFile";
import type { ObservationView } from "./useObservation";
import { useTranslation, type MessageKey } from "../../i18n";

const errorKeys: Record<ObservationErrorCode, MessageKey> = {
  inaccessible: "changes.error.inaccessible",
  git_unavailable: "changes.error.git_unavailable",
  unsafe_repository: "changes.error.unsafe_repository",
  timeout: "changes.error.timeout",
  invalid_status: "changes.error.invalid_status",
  unsupported_path_encoding: "changes.error.unsupported_path_encoding",
  resource_limit: "changes.error.resource_limit",
  unsupported_configuration: "changes.error.unsupported_configuration",
};

export function ChangedFileList({ observation, entryId, selectionGeneration, selection, onSelect, onRecheck }: {
  observation: ObservationView;
  entryId: string;
  selectionGeneration: number;
  selection: ReviewSelection | null;
  onSelect: (selection: ReviewSelection) => void;
  onRecheck?: () => void;
}) {
  const { t } = useTranslation();
  const files = observation.kind === "ready" ? observation.files : null;
  const treeFiles = useMemo(() => (files ?? []).map(changedPathTreeFile), [files]);
  let status: { title: string; description: string } | null = null;
  if (observation.kind !== "ready") {
    const title = t(observation.kind === "bare" ? "changes.noWorkingTree"
      : observation.kind === "checking" ? "changes.checking" : "changes.unavailable");
    const description = observation.kind === "bare" ? t("changes.bareDescription")
      : observation.kind === "checking" ? t("changes.waiting")
      : observation.kind === "transport_unavailable" ? t("changes.transport")
      : t("changes.retryDescription", { error: t(errorKeys[observation.errorCode]) });
    status = { title, description };
  } else if (files?.length === 0) {
    status = { title: t("changes.clean"), description: t("changes.cleanDescription") };
  }
  return <>
    <FileExplorer key={`${entryId}:${selectionGeneration}`} files={treeFiles}
      count={files?.length ?? null} treeLabel={t("changes.tree")} listLabel={t("changes.list")}
      selectedId={selection?.stablePathId} onSelect={(item) => {
        const file = files?.find((candidate) => candidate.stablePathId === item.id);
        if (!file) return;
        const category: ReviewCategory = file.unstaged !== null ? "unstaged" : file.staged !== null ? "staged" : file.untracked ? "untracked" : "unstaged";
        onSelect({ stablePathId: file.stablePathId, category, displayPath: file.displayPath });
      }}>
      {status ? <div className="changes-state" role="status"><h3>{status.title}</h3>{observation.kind === "unavailable" || observation.kind === "transport_unavailable" ? <p>{status.description}</p> : null}</div> : null}
    </FileExplorer>
    {onRecheck ? <Tooltip content={t("changes.recheckDescription")}
      trigger={<button type="button" className="retry-button" onClick={onRecheck}>{t("changes.recheck")}</button>} /> : null}
  </>;
}
