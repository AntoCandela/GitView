/** Presents the review-mode switcher; syntax themes belong to the workspace toolbar. */
import { SegmentedControl } from "../../ui/SegmentedControl";
import type { ReviewChoices, ReviewMode, LineMode } from "../appearance";
import { useTranslation } from "../../i18n";

export function ReviewControls({ choices }: { choices: ReviewChoices }) {
  const { t } = useTranslation();
  const modes = [{ value: "changes", label: t("diff.controls.changes") }, { value: "full", label: t("diff.controls.full") }] as const;
  const lines = [{ value: "scroll", label: t("diff.controls.scroll") }, { value: "wrap", label: t("diff.controls.wrap") }] as const;
  return <div className="review-controls">
    <SegmentedControl<ReviewMode> label={t("diff.controls.codeView")} value={choices.mode} options={modes}
      onChange={(mode) => choices.change({ mode })} />
    <SegmentedControl<LineMode> label={t("diff.controls.longLines")} value={choices.lineMode} options={lines}
      onChange={(lineMode) => choices.change({ lineMode })} />
  </div>;
}
