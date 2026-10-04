/** Reads only native-authorized pinned commit files and isolates every selection's completion. */

import { useEffect, useMemo, useRef, useState } from "react";
import type { CommitReviewResult } from "../../contracts/inspection";
import { appearanceTheme, useAppearanceTheme, useReviewChoices } from "../appearance";
import type { RepositoryClient } from "../../contracts/repositories";
import { useTranslation } from "../../i18n";
import { Tooltip } from "../../ui/Tooltip";
import { ReviewControls } from "./ReviewControls";
import { TextDiff } from "./text/TextDiff";
import { unavailableMessageKeys, unsupportedMessageKeys } from "./reviewOutcomeLabels";

import type { CommitReviewSelection } from "./selection";

export function CommitFileReview({ client, entryId, selectionGeneration, contextLabel, selection }: {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  contextLabel: string;
  selection: CommitReviewSelection;
}) {
  const { t } = useTranslation();
  const [attempt, setAttempt] = useState(0);
  const choices = useReviewChoices();
  const appearance = useAppearanceTheme();
  const scope = useMemo(() => ({ client, entryId, selectionGeneration, contextLabel, selection, attempt }),
    [client, entryId, selectionGeneration, contextLabel, selection, attempt]);
  const desired = useRef(scope);
  desired.current = scope;
  const [state, setState] = useState<{
    scope: typeof scope;
    result: CommitReviewResult | null;
    transportError: boolean;
  } | null>(null);
  useEffect(() => {
    let current = true;
    async function readComparison() {
      let fileId = selection.fileId;
      let result = await client.reviewCommitFile(entryId, selection.commitOid, selection.parentOid, fileId);
      if (!current || desired.current !== scope) return;
      if (result.kind === "stale_selection") {
        // A bounded native cache may evict a still-visible leaf. Labels only match a fresh native listing;
        // they never authorize a read. Renew once in this exact scope, never retry a stale renewed token.
        const authorization = await client.commitFiles(entryId, selection.commitOid, selection.parentOid);
        if (!current || desired.current !== scope) return;
        if (authorization.kind === "files" && authorization.commitOid === selection.commitOid
          && authorization.parentOid === selection.parentOid
          && (selection.parentOid === null ? authorization.parents.length === 0 : authorization.parents.includes(selection.parentOid))) {
          const matches = authorization.files.filter((file) => file.displayPath === selection.displayPath
            && file.segments.length === selection.segments.length
            && file.segments.every((segment, index) => segment === selection.segments[index]));
          if (matches.length === 1) {
            fileId = matches[0].id;
            result = await client.reviewCommitFile(entryId, selection.commitOid, selection.parentOid, fileId);
            if (!current || desired.current !== scope) return;
          }
        }
      }
      if (result.kind !== "stale_selection") {
        const identity = result.kind === "text" ? result : result.identity;
        if (identity.entryId !== entryId || identity.fileId !== fileId
          || identity.commitOid !== selection.commitOid || identity.parentOid !== selection.parentOid) {
          setState({ scope, result: { kind: "unavailable", code: "invalid_output", identity: {
            entryId, fileId, commitOid: selection.commitOid, parentOid: selection.parentOid,
            displayPath: selection.displayPath, contextLabel, fromAbsent: selection.fromAbsent, toAbsent: selection.toAbsent,
          } }, transportError: false });
          return;
        }
      }
      setState({ scope, result, transportError: false });
    }
    void readComparison().catch(() => {
      if (current && desired.current === scope) setState({ scope, result: null, transportError: true });
    });
    return () => { current = false; };
  }, [client, entryId, selection, scope, contextLabel]);
  const current = state?.scope === scope ? state : null;
  const result = current?.result;
  const identity = result?.kind === "text" ? result
    : result?.kind === "unsupported" || result?.kind === "unavailable" ? result.identity : null;
  const fromAbsent = identity?.fromAbsent ?? selection.fromAbsent;
  const toAbsent = identity?.toAbsent ?? selection.toAbsent;
  const oldEndpointLabel = selection.parentOid === null ? t("diff.endpoint.root")
    : t("diff.endpoint.parent", { oid: selection.parentOid.slice(0, 10), absent: String(fromAbsent) });
  const newEndpointLabel = t("diff.endpoint.commit", { oid: selection.commitOid.slice(0, 10), absent: String(toAbsent) });
  const title = !current ? t("diff.state.checking")
    : current.transportError ? t("diff.state.transport")
    : result?.kind === "unsupported" ? t("diff.state.unsupported")
    : result?.kind === "stale_selection" ? t("diff.state.comparisonExpired")
    : result?.kind === "unavailable" ? t("diff.state.comparisonUnavailable")
    : result?.kind === "text" && result.hunks.length === 0 ? t("diff.state.noTextDifferences") : null;
  const description = current?.transportError ? t("diff.state.retryHistorical")
    : result?.kind === "unsupported" ? t(unsupportedMessageKeys[result.reason])
    : result?.kind === "unavailable" ? t(unavailableMessageKeys[result.code])
    : result?.kind === "stale_selection" ? t("diff.state.reselectCommitted") : null;

  return <section className="file-review" aria-label={t("diff.selectedReview")} aria-description={identity?.contextLabel ?? contextLabel}
    aria-busy={!current}>
    <header className="diff-heading">
      <h2>{identity?.displayPath ?? selection.displayPath}</h2>
      <ReviewControls choices={choices} />
    </header>
    {title ? <div className="diff-state" role="status"><h3>{title}</h3>{description ? <p>{description}</p> : null}
      {current?.transportError || result?.kind === "unavailable" ? <Tooltip content={t("diff.controls.retryComparison")}
        trigger={<button type="button" className="retry-button"
          onClick={() => setAttempt((value) => value + 1)}>{t("diff.controls.retryComparison")}</button>} /> : null}
    </div> : null}
    <TextDiff key={JSON.stringify([entryId, selectionGeneration, selection.commitOid, selection.parentOid, selection.fileId])}
      review={result?.kind === "text" ? result : null} oldEndpointLabel={oldEndpointLabel} newEndpointLabel={newEndpointLabel}
      mode={choices.mode} theme={choices.theme === "match" ? appearanceTheme(appearance.theme).syntax : choices.theme} lineMode={choices.lineMode} />
  </section>;
}
