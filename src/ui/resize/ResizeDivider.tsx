/** Accessible single-axis divider with pointer capture, bounded resizing and optional center snapping. */
import { useRef, useState, type PointerEvent, type RefObject } from "react";
import { constrainResize, dividerSize, snapResizeToCenter } from "./resizeGeometry";
import { Tooltip } from "../Tooltip";
import { useTranslation } from "../../i18n";

interface ResizeDividerProps {
  label: string;
  controls: string;
  root: RefObject<HTMLElement | null>;
  axis?: "width" | "height";
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  snap?: boolean;
  className?: string;
  valueText?: string;
}

/** `value` is the leading pane's pixel size within `root`; bounds use the same coordinate space. */
export function ResizeDivider({ label, controls, root, axis = "width", value, min, max, onChange,
  snap = false, className = "", valueText }: ResizeDividerProps) {
  const { t } = useTranslation();
  const [resizing, setResizing] = useState(false);
  const drag = useRef<{ pointerId: number; offset: number } | null>(null);
  const [snapped, setSnapped] = useState(false);
  const vertical = axis === "width";
  function finish(event: PointerEvent<HTMLDivElement>) {
    if (drag.current?.pointerId !== event.pointerId) return;
    drag.current = null;
    setResizing(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    setSnapped(false);
  }
  return <Tooltip content={snap ? t("ui.resizeHint", { label }) : label} trigger={<div className={`ui-resize-divider ${vertical ? "is-vertical" : "is-horizontal"}${resizing ? " is-resizing" : ""}${snapped ? " is-snapped" : ""} ${className}`}
    role="separator" tabIndex={0} aria-label={label} aria-controls={controls}
    aria-orientation={vertical ? "vertical" : "horizontal"}
    aria-valuemin={Math.round(min)} aria-valuemax={Math.round(max)} aria-valuenow={Math.round(value)}
    aria-valuetext={valueText ?? t("ui.pixels", { count: Math.round(value) })}
    onPointerDown={(event) => {
      if (drag.current || event.button !== 0 || !event.isPrimary) return;
      const rect = root.current?.getBoundingClientRect();
      if (!rect || rect[axis] <= 0) return;
      event.preventDefault();
      event.currentTarget.focus();
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { pointerId: event.pointerId,
        offset: (vertical ? event.clientX - rect.left : event.clientY - rect.top) - value };
      setResizing(true);
    }}
    onPointerMove={(event) => {
      const start = drag.current;
      const rect = root.current?.getBoundingClientRect();
      if (!start || start.pointerId !== event.pointerId || !rect || rect[axis] <= 0) return;
      const next = (vertical ? event.clientX - rect.left : event.clientY - rect.top) - start.offset;
      const snappedValue = snap ? snapResizeToCenter(next, rect[axis]) : next;
      const constrained = constrainResize(snappedValue, min, max);
      setSnapped(snap && constrained === (rect[axis] - dividerSize) / 2);
      onChange(constrained);
    }}
    onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}
    onKeyDown={(event) => {
      const step = event.shiftKey ? 32 : 16;
      const next = event.key === (vertical ? "ArrowLeft" : "ArrowUp") ? value - step
        : event.key === (vertical ? "ArrowRight" : "ArrowDown") ? value + step
        : event.key === "Home" ? min : event.key === "End" ? max : null;
      if (next === null) return;
      event.preventDefault();
      onChange(constrainResize(next, min, max));
    }} />} />;
}
