/** Browses native-issued directory pages; partial counts and status never become read authority. */

import { useMemo } from "react";
import type { RepositoryFileSelection } from "../../../contracts/browsing";
import type { RepositoryClient } from "../../../contracts/repositories";
import { changedPathTreeFile, type ObservationView } from "../../changes";
import { FileExplorer } from "../../../ui/file-explorer/FileExplorer";
import { RefreshIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { useRepositoryFiles } from "./useRepositoryFiles";

export function RepositoryFiles({ client, entryId, enabled, observation, selected, onSelect }: {
  client: RepositoryClient;
  entryId: string;
  enabled: boolean;
  observation: ObservationView | null;
  selected: RepositoryFileSelection | null;
  onSelect: (file: RepositoryFileSelection) => void;
}) {
  const revision = observation?.kind === "ready" ? observation.observationRevision : null;
  const listing = useRepositoryFiles(client, entryId, enabled, revision);
  const nativeFiles = useMemo(() => new Map(listing.files.map((file) => [file.id, file])), [listing.files]);
  const summaryFiles = useMemo(() => observation?.kind === "ready" ? observation.files.map(changedPathTreeFile) : null, [observation]);
  const files = useMemo(() => {
    const pending = new Map(summaryFiles?.map((file) => [JSON.stringify(file.segments), file]));
    return listing.files.map((file) => {
      const changed = pending.get(JSON.stringify(file.segments));
      return changed ? { ...changed, ...file }
        : { ...file, status: summaryFiles === null ? "Status unavailable" : "Unchanged", marker: "" };
    }).sort((left, right) => left.displayPath < right.displayPath ? -1 : left.displayPath > right.displayPath ? 1 : 0);
  }, [listing.files, summaryFiles]);
  const selectedId = files.find((file) => file.displayPath === selected?.displayPath
    && file.segments.length === selected.segments.length
    && file.segments.every((segment, index) => segment === selected.segments[index]))?.id;
  const hasListing = listing.listingId !== null;
  const empty = listing.complete && files.length === 0 && listing.directories.length === 0;
  const message = !hasListing ? listing.error ?? "Loading repository files…"
    : empty ? "No files in this repository." : null;
  return <section className="file-sidebar repository-files-pane" aria-label="Repository files">
    <FileExplorer files={files} directories={listing.directories} summaryFiles={summaryFiles}
      onExpandedDirectoriesChange={listing.expandDirectories}
      treeLabel="Repository file hierarchy" listLabel="Repository file list"
      actions={<Tooltip content="Refresh files" trigger={<button type="button" className="files-expand" aria-label="Refresh files"
        onClick={listing.refresh}><RefreshIcon size={16} aria-hidden="true" /></button>} />}
      count={hasListing ? files.length : null}
      countLabel={hasListing && !listing.complete ? `${files.length} files loaded` : undefined}
      selectedId={selectedId} onSelect={(item) => {
        const file = nativeFiles.get(item.id);
        if (file && listing.listingId) onSelect({ ...file, listingId: listing.listingId });
      }}>
      {message ? <div className="changes-state" role={listing.error ? "alert" : "status"}><p>{message}</p></div> : null}
    </FileExplorer>
    {hasListing && listing.error ? <p className="repository-status-notice" role="alert">{listing.error}</p> : null}
    {hasListing && observation?.kind !== "ready" ? <p className="repository-status-notice" role="status">Git status unavailable; file markers are not current.</p> : null}
  </section>;
}
