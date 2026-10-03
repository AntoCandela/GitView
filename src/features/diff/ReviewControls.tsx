/** Presents the review-mode switcher; syntax themes belong to the workspace toolbar. */
import { SegmentedControl } from "../../ui/SegmentedControl";
import type { ReviewChoices, ReviewMode, LineMode } from "../appearance";

const modes = [{ value: "changes", label: "Changes" }, { value: "full", label: "Full file" }] as const;
const lines = [{ value: "scroll", label: "Scroll" }, { value: "wrap", label: "Wrap" }] as const;

export function ReviewControls({ choices }: { choices: ReviewChoices }) {
  return <div className="review-controls">
    <SegmentedControl<ReviewMode> label="Code view" value={choices.mode} options={modes}
      onChange={(mode) => choices.change({ mode })} />
    <SegmentedControl<LineMode> label="Long lines" value={choices.lineMode} options={lines}
      onChange={(lineMode) => choices.change({ lineMode })} />
  </div>;
}
