/** Browses native-issued directory pages; partial counts and status never become read authority. */

import { useMemo } from "react";
import type { RepositoryFileSelection } from "../../../contracts/browsing";
import type { RepositoryClient } from "../../../contracts/repositories";
import { changedPathTreeFile, type ObservationView } from "../../changes";
import { FileExplorer } from "../../../ui/file-explorer/FileExplorer";
import { RefreshIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { useRepositoryFiles } from "./useRepositoryFiles";
import { translate, useTranslation, type Locale, type MessageKey } from "../../../i18n";
import type { HistoryErrorCode } from "../../../contracts/history";
import type { ChangeTreeFile } from "../../../ui/file-explorer/ChangeTree";
import type { RepositoryFilesError } from "./useRepositoryFiles";

const listingErrorKeys: Record<HistoryErrorCode, MessageKey> = {
  inaccessible: "repo.files.error.inaccessible", git_unavailable: "repo.files.error.git_unavailable", unsafe_repository: "repo.files.error.unsafe_repository",
  timeout: "repo.files.error.timeout", invalid_output: "repo.files.error.invalid_output", resource_limit: "repo.files.error.resource_limit",
  stale_selection: "repo.files.error.stale_selection", stale_cursor: "repo.files.error.stale_cursor", missing_objects: "repo.files.error.missing_objects",
};

function filesErrorMessage(error: RepositoryFilesError, locale: Locale): string {
  return translate(locale, error.domain === "listing" ? listingErrorKeys[error.code]
    : error.domain === "transport" ? "repo.files.error.transport" : "repo.files.error.verification");
}

export function RepositoryFiles({ client, entryId, enabled, observation, selected, onSelect }: {
  client: RepositoryClient;
  entryId: string;
  enabled: boolean;
  observation: ObservationView | null;
  selected: RepositoryFileSelection | null;
  onSelect: (file: RepositoryFileSelection) => void;
}) {
  const { locale, t } = useTranslation();
  const revision = observation?.kind === "ready" ? observation.observationRevision : null;
  const listing = useRepositoryFiles(client, entryId, enabled, revision);
  const nativeFiles = useMemo(() => new Map(listing.files.map((file) => [file.id, file])), [listing.files]);
  const summaryFiles = useMemo(() => observation?.kind === "ready" ? observation.files.map(changedPathTreeFile) : null, [observation]);
  const files = useMemo<ChangeTreeFile[]>(() => {
    const pending = new Map(summaryFiles?.map((file) => [JSON.stringify(file.segments), file]));
    return listing.files.map((file): ChangeTreeFile => {
      const changed = pending.get(JSON.stringify(file.segments));
      return changed ? { ...changed, ...file }
        : { ...file, statuses: [{ kind: summaryFiles === null ? "unavailable" : "unchanged" }], marker: "" };
    }).sort((left, right) => left.displayPath < right.displayPath ? -1 : left.displayPath > right.displayPath ? 1 : 0);
  }, [listing.files, summaryFiles]);
  const selectedId = files.find((file) => file.displayPath === selected?.displayPath
    && file.segments.length === selected.segments.length
    && file.segments.every((segment, index) => segment === selected.segments[index]))?.id;
  const hasListing = listing.listingId !== null;
  const empty = listing.complete && files.length === 0 && listing.directories.length === 0;
  const error = listing.error ? filesErrorMessage(listing.error, locale) : null;
  const message = !hasListing ? error ?? t("repo.files.loading")
    : empty ? t("repo.files.empty") : null;
  return <section className="file-sidebar repository-files-pane" aria-label={t("repo.files.label")}>
    <FileExplorer files={files} directories={listing.directories} summaryFiles={summaryFiles}
      onExpandedDirectoriesChange={listing.expandDirectories}
      treeLabel={t("repo.files.tree")} listLabel={t("repo.files.list")}
      actions={<Tooltip content={t("repo.files.refresh")} trigger={<button type="button" className="files-expand" aria-label={t("repo.files.refresh")}
        onClick={listing.refresh}><RefreshIcon size={16} aria-hidden="true" /></button>} />}
      count={hasListing ? files.length : null}
      countLabel={hasListing && !listing.complete ? t("repo.files.loaded", { count: files.length }) : undefined}
      selectedId={selectedId} onSelect={(item) => {
        const file = nativeFiles.get(item.id);
        if (file && listing.listingId) onSelect({ ...file, listingId: listing.listingId });
      }}>
      {message ? <div className="changes-state" role={listing.error ? "alert" : "status"}><p>{message}</p></div> : null}
    </FileExplorer>
    {hasListing && error ? <p className="repository-status-notice" role="alert">{error}</p> : null}
    {hasListing && observation?.kind !== "ready" ? <p className="repository-status-notice" role="status">{t("repo.files.statusUnavailable")}</p> : null}
  </section>;
}
