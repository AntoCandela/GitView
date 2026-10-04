/** Keeps three stable panes in configurable slots, with bounded pointer and keyboard splitters. */

import { useEffect, useId, useRef, useState, type CSSProperties, type PointerEvent, type ReactNode } from "react";
import { usePanelLayout } from "../../ui/resize/panelLayout";
import { dividerSize, snapResizeToCenter } from "../../ui/resize/resizeGeometry";
import { Tooltip } from "../../ui/Tooltip";
import { defaultWorkbenchLayout, workbenchLayouts, type WorkbenchLayoutId } from "./workbenchLayout";
import { useTranslation } from "../../i18n";

type Axis = "width" | "height";
type ResizeMode = Axis | "both";
function bounds(available: number, axis: Axis) {
  return {
    min: Math.min(128, available * 0.4),
    max: Math.max(0, available - Math.min(axis === "width" ? 180 : 128, available * 0.45) - dividerSize),
  };
}
function constrain(value: number, available: number, axis: Axis) {
  const { min, max } = bounds(available, axis);
  return Math.max(Math.min(min, max), Math.min(value, max));
}

export function ResizableWorkbench({ files, history, comparison, layout = defaultWorkbenchLayout }: {
  files: ReactNode; history: ReactNode; comparison: ReactNode; layout?: WorkbenchLayoutId;
}) {
  const { t } = useTranslation();
  const root = useRef<HTMLElement>(null);
  const { resetVersion } = usePanelLayout();
  const mountedResetVersion = useRef(resetVersion);
  const panePrefix = useId();
  const [available, setAvailable] = useState({ width: 600, height: 600 });
  const [width, setWidth] = useState(240);
  const [height, setHeight] = useState(270);
  const [resizing, setResizing] = useState<ResizeMode | null>(null);
  const drag = useRef<{
    pointerId: number; mode: ResizeMode; offsetX: number; offsetY: number;
  } | null>(null);
  useEffect(() => {
    const element = root.current;
    if (!element) return;
    let initialized = false;
    const measure = () => {
      const size = element.getBoundingClientRect();
      if (size.width <= 0 || size.height <= 0) return;
      setAvailable({ width: size.width, height: size.height });
      const preserveSize = initialized;
      const defaultWidth = resetVersion !== mountedResetVersion.current
        ? (size.width - dividerSize) / 2
        : size.width <= 440 ? 128 : size.width <= 700 ? 160 : 240;
      setWidth((previous) => constrain(preserveSize ? previous : defaultWidth, size.width, "width"));
      setHeight((previous) => constrain(preserveSize ? previous : size.height * 0.45, size.height, "height"));
      initialized = true;
    };
    drag.current = null;
    setResizing(null);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [resetVersion]);
  const arrangement = workbenchLayouts.find((candidate) => candidate.id === layout)!;
  const snappedWidth = resizing !== null && resizing !== "height" && width === (available.width - dividerSize) / 2;
  const snappedHeight = resizing !== null && resizing !== "width" && height === (available.height - dividerSize) / 2;
  const paneId = (panel: string) => `${panePrefix}-${panel}`;
  const setSize = (axis: Axis, value: number) => {
    const measured = root.current?.getBoundingClientRect()[axis] ?? available[axis];
    const next = constrain(value, measured > 0 ? measured : available[axis], axis);
    if (axis === "width") setWidth(next);
    else setHeight(next);
  };
  const finishDrag = (event: PointerEvent<HTMLElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return;
    drag.current = null;
    setResizing(null);
  };

  function pointerHandlers(mode: ResizeMode) {
    return {
      onPointerDown(event: PointerEvent<HTMLElement>) {
        if (drag.current || event.button !== 0 || !event.isPrimary) return;
        const rectangle = root.current?.getBoundingClientRect();
        if (!rectangle) return;
        event.preventDefault();
        event.currentTarget.focus();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = {
          pointerId: event.pointerId, mode,
          offsetX: event.clientX - rectangle.left - width,
          offsetY: event.clientY - rectangle.top - height,
        };
        setResizing(mode);
      },
      onPointerMove(event: PointerEvent<HTMLElement>) {
        const start = drag.current;
        if (!start || start.pointerId !== event.pointerId) return;
        const rectangle = root.current?.getBoundingClientRect();
        if (!rectangle || rectangle.width <= 0 || rectangle.height <= 0) return;
        setAvailable({ width: rectangle.width, height: rectangle.height });
        if (start.mode !== "height") setSize("width", snapResizeToCenter(event.clientX - rectangle.left - start.offsetX, rectangle.width));
        if (start.mode !== "width") setSize("height", snapResizeToCenter(event.clientY - rectangle.top - start.offsetY, rectangle.height));
      },
      onPointerUp(event: PointerEvent<HTMLElement>) {
        if (drag.current?.pointerId !== event.pointerId) return;
        finishDrag(event);
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
      },
      onPointerCancel: finishDrag,
      onLostPointerCapture: finishDrag,
    };
  }

  function splitter(axis: Axis) {
    const limits = bounds(available[axis], axis);
    const value = axis === "width" ? width : height;
    const snapped = axis === "width" ? snappedWidth : snappedHeight;
    const label = t(axis === "height" ? "app.resizeRows" : arrangement.left === "files" ? "app.resizeFiles"
      : arrangement.left === "history" ? "app.resizeGraph" : "app.resizeComparison");
    return <Tooltip content={t("ui.resizeHint", { label })} trigger={<div className={`workbench-divider ${axis === "width" ? "column-divider" : "row-divider"}${snapped ? " is-snapped" : ""}`}
      role="separator" tabIndex={0} aria-label={label}
      aria-orientation={axis === "width" ? "vertical" : "horizontal"}
      aria-controls={paneId(axis === "width" ? arrangement.left : arrangement.top)}
      aria-valuemin={Math.round(Math.min(limits.min, limits.max))} aria-valuemax={Math.round(limits.max)}
      aria-valuenow={Math.round(value)} aria-valuetext={t("ui.pixels", { count: Math.round(value) })}
      {...pointerHandlers(axis)}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 32 : 16;
        const next = event.key === (axis === "width" ? "ArrowLeft" : "ArrowUp") ? value - step
          : event.key === (axis === "width" ? "ArrowRight" : "ArrowDown") ? value + step
          : event.key === "Home" ? limits.min : event.key === "End" ? limits.max : null;
        if (next === null) return;
        event.preventDefault();
        setSize(axis, next);
      }}
    />} />;
  }

  return <section ref={root} className={`workbench${resizing ? ` is-resizing resizing-${resizing}` : ""}`}
    aria-label={t("app.workbench")} data-workbench-layout={layout}
    style={{
      "--workbench-left-width": `${width}px`, "--workbench-top-height": `${height}px`,
      gridTemplateAreas: `"${arrangement.top} ${arrangement.top} ${arrangement.top}" "rows rows rows" "${arrangement.left} columns ${arrangement.right}"`,
    } as CSSProperties}>
    <div id={paneId("comparison")} className="comparison-panel workbench-pane" data-panel="comparison">{comparison}</div>
    <aside id={paneId("files")} className="file-sidebar workbench-pane" data-panel="files" aria-label={t("app.panel.files")}>{files}</aside>
    <div id={paneId("history")} className="history-panel workbench-pane" data-panel="history"
      style={{
        "--history-panel-width": `${arrangement.top === "history" ? available.width : arrangement.left === "history" ? width : available.width - width - dividerSize}px`,
        "--history-panel-height": `${arrangement.top === "history" ? height : available.height - height - dividerSize}px`,
      } as CSSProperties}>{history}</div>
    {splitter("width")}
    {splitter("height")}
    <Tooltip content={t("app.resizeAllHint")} trigger={<button type="button" className={`workbench-junction${snappedWidth || snappedHeight ? " is-snapped" : ""}`} aria-label={t("app.resizeAll")}
      aria-controls={`${paneId("comparison")} ${paneId("files")} ${paneId("history")}`}
      aria-description={t("app.resizeAllDescription")}
      {...pointerHandlers("both")}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 32 : 16;
        if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
          event.preventDefault();
          setSize("width", width + (event.key === "ArrowLeft" ? -step : step));
        } else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault();
          setSize("height", height + (event.key === "ArrowUp" ? -step : step));
        } else if (event.key === "Home" || event.key === "End") {
          event.preventDefault();
          const limit = event.key === "Home" ? "min" : "max";
          setSize("width", bounds(available.width, "width")[limit]);
          setSize("height", bounds(available.height, "height")[limit]);
        }
      }} />} />
  </section>;
}
