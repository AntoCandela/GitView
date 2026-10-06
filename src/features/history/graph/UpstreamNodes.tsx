/** Renders virtual upstream summaries without inventing commit ancestry or calculating Git ranges. */
import type { RefObject } from "react";
import type { UpstreamState } from "../../../contracts/history";
import type { RepositoryClient } from "../../../contracts/repositories";
import type { CommitComparisonControls } from "../../diff";
import { useTranslation } from "../../../i18n";
import { IncomingIcon, OutgoingIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { CommitFiles } from "../CommitFiles";

export function UpstreamNodes({ state, selected, onSelect, client, entryId, selectionGeneration, comparison, virtualScrollRef }: {
  state: UpstreamState; selected: string | null; onSelect: (id: string) => void;
  client: RepositoryClient; entryId: string; selectionGeneration: number;
  comparison?: CommitComparisonControls; virtualScrollRef: RefObject<HTMLElement | null>;
}) {
  const { t } = useTranslation();
  const upstream = state.upstream ?? "";
  return <div className="history-upstream">
    {state.freshness === "stale" && <p className="history-upstream-status" role="status">{t("history.upstream.stale")}</p>}
    {state.state !== "ready" && <p className="history-upstream-status">{t(`history.upstream.${state.state}`)}</p>}
    {state.state === "ready" && state.ahead === 0 && state.behind === 0 && <p className="history-upstream-status">{t("history.upstream.synchronized", { upstream })}</p>}
    {(["outgoing", "incoming"] as const).map((direction) => {
      const count = direction === "incoming" ? state.behind : state.ahead;
      if (!count) return null;
      const range = state[direction];
      const id = `upstream-${direction}`;
      const expanded = selected === id;
      const Icon = direction === "incoming" ? IncomingIcon : OutgoingIcon;
      return <div key={direction} className={`history-upstream-node history-upstream-${direction}`} data-selected={expanded}>
        <Tooltip content={<><strong>{state.branch} / {upstream}</strong><p>{t(`history.upstream.${direction}Hint`)}</p>
          {!range && <p>{t("history.upstream.noBase")}</p>}</>}
          trigger={<button type="button" className="history-upstream-trigger" aria-expanded={expanded}
            aria-label={t(`history.upstream.${direction}`, { count })} disabled={!range} onClick={() => onSelect(id)}>
            <span className="history-upstream-marker" aria-hidden="true"><Icon size={12} /></span>
            <span>{t(`history.upstream.${direction}`, { count })}</span>
          </button>} />
        {expanded && range && <CommitFiles key={range.token} client={client} entryId={entryId}
          selectionGeneration={selectionGeneration} upstream={{ range, direction }} comparison={comparison} virtualScrollRef={virtualScrollRef} />}
      </div>;
    })}
  </div>;
}
