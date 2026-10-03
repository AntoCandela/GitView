/** Loads actual parent-specific committed paths; late replies never replace a newer comparison. */

import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { HistoryCommit } from "../../contracts/history";
import type { CommitFilesResult, CommittedFile } from "../../contracts/inspection";
import type { RepositoryClient } from "../../contracts/repositories";
import type { CommitComparisonControls } from "../diff";
import { ChangeTree } from "../../ui/file-explorer/ChangeTree";
import { Tooltip } from "../../ui/Tooltip";

const kindLabels: Record<CommittedFile["kind"], string> = {
  added: "Added", modified: "Modified", deleted: "Deleted", type_change: "Type change",
};
const kindMarkers: Record<CommittedFile["kind"], string> = {
  added: "A", modified: "M", deleted: "D", type_change: "T",
};

export function CommitFiles({ client, entryId, selectionGeneration, commit, comparison, virtualScrollRef }: {
  client: RepositoryClient; entryId: string; selectionGeneration: number; commit: HistoryCommit;
  comparison?: CommitComparisonControls;
  virtualScrollRef?: RefObject<HTMLElement | null>;
}) {
  const context = useMemo(() => ({ client, entryId, selectionGeneration, oid: commit.oid }),
    [client, entryId, selectionGeneration, commit.oid]);
  const [parentChoice, setParentChoice] = useState<{ context: typeof context; oid: string } | null>(null);
  const parent = parentChoice?.context === context ? parentChoice.oid : null;
  const scope = useMemo(() => ({ context, parent }), [context, parent]);
  const desired = useRef(scope);
  desired.current = scope;
  const [state, setState] = useState<{ scope: typeof scope; result: CommitFilesResult | null; transportError: boolean } | null>(null);
  const [knownParents, setKnownParents] = useState<{ context: typeof context; parents: string[] } | null>(null);
  useEffect(() => {
    let current = true;
    void client.commitFiles(entryId, commit.oid, parent).then((result) => {
      if (!current || desired.current !== scope) return;
      if (result.kind === "files" && (result.commitOid !== commit.oid
        || result.parentOid !== (parent ?? result.parents[0] ?? null)
        || (result.parentOid !== null && !result.parents.includes(result.parentOid)))) {
        setState({ scope, result: { kind: "error", code: "invalid_output", message: "The committed files do not match this comparison." }, transportError: false });
        return;
      }
      if (result.kind === "files") setKnownParents({ context, parents: result.parents });
      setState({ scope, result, transportError: false });
    }).catch(() => {
      if (current && desired.current === scope) setState({ scope, result: null, transportError: true });
    });
    return () => { current = false; };
  }, [client, entryId, commit.oid, parent, scope, context]);
  const current = state?.scope === scope ? state : null;
  const result = current?.result;
  const parents = knownParents?.context === context ? knownParents.parents : commit.parents.map((item) => item.oid);
  const files = useMemo(() => result?.kind === "files" ? result.files.map((file) => ({
    id: file.id, displayPath: file.displayPath, segments: file.segments,
    status: kindLabels[file.kind], marker: kindMarkers[file.kind],
  })) : [], [result]);
  return <section className="history-files" aria-label={`Changed files for commit ${commit.oid}`} aria-busy={!current}>
    {parents.length > 1 && <label className="history-parent-choice">Compare with
      <Tooltip content="Choose the parent commit to compare against" trigger={<select aria-label="Comparison parent" value={parent ?? parents[0]} onChange={(event) => {
        comparison?.onInvalidate();
        setParentChoice({ context, oid: event.target.value });
      }}>
        {parents.map((oid, index) => <option key={oid} value={oid}>Parent {index + 1} · {oid.slice(0, 7)}</option>)}
      </select>} />
    </label>}
    {!current && <p className="history-notice" role="status">Loading changed files…</p>}
    {current?.transportError && <p className="history-notice history-error" role="alert">Desktop connection interrupted. Collapse and reopen to retry.</p>}
    {result && result.kind !== "files" && <p className="history-notice history-error" role="alert">{result.message}</p>}
    {result?.kind === "files" && <>
      {result.parentOid === null && <p className="history-baseline">Compared with the empty tree · root commit</p>}
      {files.length > 0 ? <ChangeTree key={result.parentOid ?? "root"} files={files} label="Committed file hierarchy"
        virtualScrollRef={virtualScrollRef}
        selectedId={comparison?.selection?.commitOid === result.commitOid
          && comparison.selection.parentOid === result.parentOid ? comparison.selection.fileId : undefined}
        onSelect={comparison ? (item) => {
          const file = result.files.find((candidate) => candidate.id === item.id);
          if (!file) return;
          comparison.onSelect({ fileId: file.id, commitOid: result.commitOid, parentOid: result.parentOid,
            displayPath: file.displayPath, segments: file.segments, fromAbsent: file.kind === "added", toAbsent: file.kind === "deleted" });
        } : undefined} />
        : <p className="history-notice" role="status">No changed files for this comparison.</p>}
    </>}
  </section>;
}
