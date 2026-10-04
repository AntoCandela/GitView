/** Presents category endpoints and distinct live review outcomes without treating failures as empty diffs. */

import { useRef, type ReactNode } from "react";
import type { ReviewCategory, ReviewIdentity, ReviewSelection } from "../../contracts/diff";
import type { RepositoryClient } from "../../contracts/repositories";
import type { PresentationInput } from "../../contracts/companion";
import { useTranslation } from "../../i18n";
import { Tooltip } from "../../ui/Tooltip";
import { appearanceTheme, useAppearanceTheme, useReviewChoices } from "../appearance";
import type { ObservationView } from "../changes";
import { ReviewControls } from "./ReviewControls";
import { TextDiff } from "./text/TextDiff";
import { useFileReview, type LiveReviewAuthority } from "./useFileReview";
import { unavailableMessageKeys, unsupportedMessageKeys } from "./reviewOutcomeLabels";

interface FileReviewProps {
  client: Pick<RepositoryClient, "reviewFile">;
  entryId: string;
  selectionGeneration: number;
  contextLabel: string;
  observation: ObservationView;
  selection: ReviewSelection | null;
  categories: ReviewCategory[];
  onCategoryChange: (category: ReviewCategory) => void;
  presentation?: Pick<PresentationInput, "review" | "appearanceTheme">;
  enabled?: boolean;
  outcome?: "no_remaining";
  onVerified?: (authority: LiveReviewAuthority) => void;
  initialAuthority?: LiveReviewAuthority;
}

/** Compact callers supply immutable presentation and never mount preference-owning hooks. */
export function FileReview(props: FileReviewProps) {
  return props.presentation ? <PresentedFileReview {...props} presentation={props.presentation} /> : <PreferredFileReview {...props} />;
}

function PreferredFileReview(props: FileReviewProps) {
  const choices = useReviewChoices();
  const appearance = useAppearanceTheme();
  return <PresentedFileReview {...props} presentation={{ review: choices, appearanceTheme: appearance.theme }} controls={<ReviewControls choices={choices} />} />;
}

function PresentedFileReview({ client, entryId, selectionGeneration, contextLabel, observation, selection, categories, onCategoryChange,
  presentation, controls, enabled = true, outcome, onVerified, initialAuthority }: FileReviewProps & { presentation: Pick<PresentationInput, "review" | "appearanceTheme">; controls?: ReactNode }) {
  const { t } = useTranslation();
  const requestedView = useFileReview(client, entryId, selectionGeneration, observation, selection, enabled && outcome !== "no_remaining", onVerified, initialAuthority);
  const view = outcome === "no_remaining" ? { kind: "no_remaining" as const } : requestedView;
  const choices = presentation.review;
  const previousIdentity = useRef<{
    client: Pick<RepositoryClient, "reviewFile">; entryId: string; generation: number; stablePathId: string; category: ReviewCategory; identity: ReviewIdentity;
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
  const fromLabel = t("diff.endpoint.from", { endpoint: from, absent: String(identity?.fromAbsent ?? false) });
  const toLabel = t("diff.endpoint.to", { endpoint: to, absent: String(identity?.toAbsent ?? false) });
  const title = view.kind === "checking" ? t("diff.state.checking")
    : view.kind === "updating" ? t("diff.state.updating")
    : view.kind === "no_remaining" ? t("diff.state.noRemaining")
    : view.kind === "unsupported" ? t("diff.state.unsupported")
    : view.kind === "transport_unavailable" ? t("diff.state.transport")
    : view.kind === "unavailable" || view.kind === "observation_unavailable" ? t("diff.state.comparisonUnavailable")
    : view.hunks.length === 0 ? t("diff.state.noTextDifferences") : null;
  const description = view.kind === "unsupported" ? t(unsupportedMessageKeys[view.reason])
    : view.kind === "unavailable" ? t(unavailableMessageKeys[view.code])
    : view.kind === "transport_unavailable" ? t("diff.state.reconnecting")
    : view.kind === "observation_unavailable" ? t("diff.state.waitingObservation")
    : view.kind === "updating" ? t("diff.state.lastVerified") : null;

  return <section className="file-review" aria-label={t("diff.selectedReview")} aria-description={identity?.contextLabel ?? contextLabel}
    aria-busy={view.kind === "checking" || view.kind === "updating"}>
    <header className="diff-heading">
      <h2>{identity?.displayPath ?? selection.displayPath}</h2>
      <div className="diff-categories" role="group" aria-label={t("diff.category.group")}>
        {(categories.length ? categories : [selection.category]).map((category) => <Tooltip key={category}
          content={t("diff.category.show", { category })} trigger={<button type="button"
            aria-label={t("diff.category.comparison", { category })} aria-pressed={selection.category === category}
            onClick={() => onCategoryChange(category)}>{t("diff.category.label", { category })}</button>} />)}
      </div>
      {controls}
    </header>
    {title ? <div className={`diff-state ${view.kind}`} role="status"><h3>{title}</h3>{description ? <p>{description}</p> : null}
      {view.kind === "unavailable" ? <p>{t("diff.state.checkingAgain")}</p> : null}
    </div> : null}
    <TextDiff key={JSON.stringify([entryId, selectionGeneration, selection.stablePathId, selection.category])}
      review={view.kind === "text" ? view : view.kind === "updating" ? view.previous : null} updating={view.kind === "updating"}
      oldEndpointLabel={fromLabel} newEndpointLabel={toLabel} mode={choices.mode}
      theme={choices.theme === "match" ? appearanceTheme(presentation.appearanceTheme).syntax : choices.theme} lineMode={choices.lineMode} />
  </section>;
}
