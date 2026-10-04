/** Reads native-authorized working files without inventing a changed-file comparison. */

import { useEffect, useMemo, useRef, useState } from "react";
import type { RepositoryFileResult, RepositoryFileSelection } from "../../contracts/browsing";
import type { RepositoryClient } from "../../contracts/repositories";
import { useTranslation } from "../../i18n";
import { SegmentedControl } from "../../ui/SegmentedControl";
import { Tooltip } from "../../ui/Tooltip";
import { appearanceTheme, useAppearanceTheme, useReviewChoices, type LineMode } from "../appearance";
import { TextDiff } from "./text/TextDiff";
import { unavailableMessageKeys, unsupportedMessageKeys } from "./reviewOutcomeLabels";

export function RepositoryFileReview({ client, entryId, selectionGeneration, contextLabel, selection }: {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  contextLabel: string;
  selection: RepositoryFileSelection;
}) {
  const { t } = useTranslation();
  const lines = [{ value: "scroll", label: t("diff.controls.scroll") }, { value: "wrap", label: t("diff.controls.wrap") }] as const;
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
  const title = !current ? t("diff.state.loadingWorkingFile")
    : current.transportError ? t("diff.state.transport")
    : result?.kind === "unsupported" ? t("diff.state.unsupported")
    : result?.kind === "unavailable" ? t("diff.state.previewUnavailable")
    : result?.kind === "stale_selection" ? t("diff.state.fileExpired") : null;
  const description = current?.transportError ? t("diff.state.retryWorkingFile")
    : result?.kind === "unsupported" ? t(unsupportedMessageKeys[result.reason])
    : result?.kind === "unavailable" ? t(unavailableMessageKeys[result.code])
    : result?.kind === "stale_selection" ? t("diff.state.refreshFiles") : null;
  const review = useMemo(() => result?.kind === "text" ? {
    displayPath: result.displayPath, fromContent: "", toContent: result.content,
    fromAbsent: true, toAbsent: false, from: "absent" as const, to: "working_files" as const, hunks: [],
  } : null, [result]);

  return <section className="file-review" aria-label={t("diff.selectedReview")} aria-description={contextLabel} aria-busy={!current}>
    <header className="diff-heading">
      <h2>{displayPath}</h2>
      <div className="review-controls">
        <SegmentedControl<LineMode> label={t("diff.controls.longLines")} value={choices.lineMode} options={lines}
          onChange={(lineMode) => choices.change({ lineMode })} />
      </div>
    </header>
    {title ? <div className="diff-state" role="status"><h3>{title}</h3>{description ? <p>{description}</p> : null}
      {current?.transportError || result?.kind === "unavailable" ? <Tooltip content={t("diff.controls.retryWorkingPreview")}
        trigger={<button type="button" className="retry-button"
          onClick={() => setAttempt((value) => value + 1)}>{t("diff.controls.retryPreview")}</button>} /> : null}
    </div> : null}
    <TextDiff key={JSON.stringify([entryId, selectionGeneration, listingId, id])} review={review}
      oldEndpointLabel={t("diff.endpoint.noComparison")} newEndpointLabel={t("diff.endpoint.workingFiles")} mode="full"
      theme={choices.theme === "match" ? appearanceTheme(appearance.theme).syntax : choices.theme} lineMode={choices.lineMode} />
  </section>;
}
