/** Reads native-authorized working files without inventing a changed-file comparison. */

import { useEffect, useMemo, useRef, useState } from "react";
import type { RepositoryFileResult, RepositoryFileSelection } from "../../contracts/browsing";
import type { RepositoryClient } from "../../contracts/repositories";
import { SegmentedControl } from "../../ui/SegmentedControl";
import { Tooltip } from "../../ui/Tooltip";
import { appearanceTheme, useAppearanceTheme, useReviewChoices, type LineMode } from "../appearance";
import { TextDiff } from "./text/TextDiff";
import { unavailableLabels, unsupportedLabels } from "./reviewOutcomeLabels";

const lines = [{ value: "scroll", label: "Scroll" }, { value: "wrap", label: "Wrap" }] as const;

export function RepositoryFileReview({ client, entryId, selectionGeneration, contextLabel, selection }: {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  contextLabel: string;
  selection: RepositoryFileSelection;
}) {
  const [attempt, setAttempt] = useState(0);
  const choices = useReviewChoices();
  const appearance = useAppearanceTheme();
  const { id, listingId, displayPath } = selection;
  const scope = useMemo(() => ({ client, entryId, selectionGeneration, listingId, id, displayPath, attempt }),
    [client, entryId, selectionGeneration, listingId, id, displayPath, attempt]);
  const desired = useRef(scope);
  desired.current = scope;
  const [state, setState] = useState<{
    scope: typeof scope; result: RepositoryFileResult | null; transportError: boolean;
  } | null>(null);
  useEffect(() => {
    let current = true;
    void client.reviewRepositoryFile(entryId, listingId, id).then((reply) => {
      if (!current || desired.current !== scope) return;
      const result: RepositoryFileResult = reply.kind === "text"
        && (reply.entryId !== entryId || reply.listingId !== listingId || reply.fileId !== id || reply.displayPath !== displayPath)
        ? { kind: "unavailable", code: "invalid_output" } : reply;
      setState({ scope, result, transportError: false });
    }).catch(() => {
      if (current && desired.current === scope) setState({ scope, result: null, transportError: true });
    });
    return () => { current = false; };
  }, [client, entryId, listingId, id, displayPath, scope]);
  const current = state?.scope === scope ? state : null;
  const result = current?.result;
  const title = !current ? "Loading working file…"
    : current.transportError ? "Desktop connection interrupted"
    : result?.kind === "unsupported" ? "Preview unsupported"
    : result?.kind === "unavailable" ? "Preview unavailable"
    : result?.kind === "stale_selection" ? "File selection expired" : null;
  const description = current?.transportError ? "Retry reading the selected working file."
    : result?.kind === "unsupported" ? unsupportedLabels[result.reason]
    : result?.kind === "unavailable" ? unavailableLabels[result.code]
    : result?.kind === "stale_selection" ? "Refresh the repository files and select the file again." : null;
  const review = useMemo(() => result?.kind === "text" ? {
    displayPath: result.displayPath, fromContent: "", toContent: result.content,
    fromAbsent: true, toAbsent: false, from: "absent" as const, to: "working_files" as const, hunks: [],
  } : null, [result]);

  return <section className="file-review" aria-label="Selected file review" aria-description={contextLabel} aria-busy={!current}>
    <header className="diff-heading">
      <h2>{displayPath}</h2>
      <div className="review-controls">
        <SegmentedControl<LineMode> label="Long lines" value={choices.lineMode} options={lines}
          onChange={(lineMode) => choices.change({ lineMode })} />
      </div>
    </header>
    {title ? <div className="diff-state" role="status"><h3>{title}</h3>{description ? <p>{description}</p> : null}
      {current?.transportError || result?.kind === "unavailable" ? <Tooltip content="Retry working file preview"
        trigger={<button type="button" className="retry-button"
          onClick={() => setAttempt((value) => value + 1)}>Retry preview</button>} /> : null}
    </div> : null}
    <TextDiff key={JSON.stringify([entryId, selectionGeneration, listingId, id])} review={review}
      oldEndpointLabel="No comparison endpoint" newEndpointLabel="Working files" mode="full"
      theme={choices.theme === "match" ? appearanceTheme(appearance.theme).syntax : choices.theme} lineMode={choices.lineMode} />
  </section>;
}
