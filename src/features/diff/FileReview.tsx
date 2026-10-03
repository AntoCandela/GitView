/** Presents category endpoints and distinct live review outcomes without treating failures as empty diffs. */

import { useRef } from "react";
import type { ReviewCategory, ReviewIdentity, ReviewSelection } from "../../contracts/diff";
import type { RepositoryClient } from "../../contracts/repositories";
import { Tooltip } from "../../ui/Tooltip";
import { appearanceTheme, useAppearanceTheme, useReviewChoices } from "../appearance";
import type { ObservationView } from "../changes";
import { ReviewControls } from "./ReviewControls";
import { TextDiff } from "./text/TextDiff";
import { useFileReview } from "./useFileReview";
import { unavailableLabels, unsupportedLabels } from "./reviewOutcomeLabels";

const categoryLabels: Record<ReviewCategory, string> = { staged: "Staged", unstaged: "Unstaged", untracked: "Untracked" };

export function FileReview({ client, entryId, selectionGeneration, contextLabel, observation, selection, categories, onCategoryChange }: {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  contextLabel: string;
  observation: ObservationView;
  selection: ReviewSelection | null;
  categories: ReviewCategory[];
  onCategoryChange: (category: ReviewCategory) => void;
}) {
  const view = useFileReview(client, entryId, selectionGeneration, observation, selection);
  const choices = useReviewChoices();
  const appearance = useAppearanceTheme();
  const previousIdentity = useRef<{
    client: RepositoryClient; entryId: string; generation: number; stablePathId: string; category: ReviewCategory; identity: ReviewIdentity;
  } | null>(null);
  if (!selection || view.kind === "idle") return null;
  const currentIdentity = view.kind === "text" ? view : view.kind === "updating" ? view.previous
    : view.kind === "unsupported" || view.kind === "unavailable" ? view.identity : null;
  if (currentIdentity) previousIdentity.current = {
    client, entryId, generation: selectionGeneration, stablePathId: selection.stablePathId, category: selection.category, identity: currentIdentity,
  };
  const retained = previousIdentity.current;
  const identity = currentIdentity ?? (view.kind === "no_remaining" && retained?.client === client
    && retained.entryId === entryId && retained.generation === selectionGeneration
    && retained.stablePathId === selection.stablePathId && retained.category === selection.category ? retained.identity : null);
  const from = identity?.from ?? (selection.category === "staged" ? "HEAD" : selection.category === "unstaged" ? "index" : "absent");
  const to = identity?.to ?? (selection.category === "staged" ? "index" : "working_files");
  const fromLabel = from === "absent" ? "Absent (new file)" : `${from === "HEAD" ? "HEAD" : "Index"}${identity?.fromAbsent ? " · absent" : ""}`;
  const toLabel = `${to === "index" ? "Index" : "Working files"}${identity?.toAbsent ? " · absent (deleted file)" : ""}`;
  const title = view.kind === "checking" ? "Checking comparison…"
    : view.kind === "updating" ? "Updating comparison…"
    : view.kind === "no_remaining" ? "No remaining changes"
    : view.kind === "unsupported" ? "Preview unsupported"
    : view.kind === "transport_unavailable" ? "Desktop connection interrupted"
    : view.kind === "unavailable" || view.kind === "observation_unavailable" ? "Comparison unavailable"
    : view.hunks.length === 0 ? "No text differences" : null;
  const description = view.kind === "unsupported" ? unsupportedLabels[view.reason]
    : view.kind === "unavailable" ? `${unavailableLabels[view.code]} Checking again automatically.`
    : view.kind === "transport_unavailable" ? "Reconnecting automatically. No current comparison is available."
    : view.kind === "observation_unavailable" ? "Waiting for a current working-tree observation."
    : view.kind === "updating" ? "Last verified comparison" : null;

  return <section className="file-review" aria-label="Selected file review" aria-description={identity?.contextLabel ?? contextLabel}
    aria-busy={view.kind === "checking" || view.kind === "updating"}>
    <header className="diff-heading">
      <h2>{identity?.displayPath ?? selection.displayPath}</h2>
      <div className="diff-categories" role="group" aria-label="Comparison category">
        {(categories.length ? categories : [selection.category]).map((category) => <Tooltip key={category}
          content={`Show ${categoryLabels[category].toLowerCase()} comparison`} trigger={<button type="button"
            aria-label={`${categoryLabels[category]} comparison`} aria-pressed={selection.category === category}
            onClick={() => onCategoryChange(category)}>{categoryLabels[category]}</button>} />)}
      </div>
      <ReviewControls choices={choices} />
    </header>
    {title ? <div className={`diff-state ${view.kind}`} role="status"><h3>{title}</h3>{description ? <p>{description}</p> : null}</div> : null}
    <TextDiff key={JSON.stringify([entryId, selectionGeneration, selection.stablePathId, selection.category])}
      review={view.kind === "text" ? view : view.kind === "updating" ? view.previous : null} updating={view.kind === "updating"}
      oldEndpointLabel={fromLabel} newEndpointLabel={toLabel} mode={choices.mode}
      theme={choices.theme === "match" ? appearanceTheme(appearance.theme).syntax : choices.theme} lineMode={choices.lineMode} />
  </section>;
}
