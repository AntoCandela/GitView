/** Provides one opaque, portaled hover/focus tooltip; its trigger never adds a competing native title. */

import { cloneElement, useEffect, useId, useRef, useState, type ReactElement, type ReactNode } from "react";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";

export function Tooltip({
  trigger,
  content,
}: {
  trigger: ReactElement<Record<string, unknown>>;
  content: ReactNode;
}) {
  const generatedId = useId();
  const id = typeof trigger.props.id === "string" ? trigger.props.id : generatedId;
  const [open, setOpen] = useState(false);
  const disabled = trigger.props.disabled === true;
  const hoverTimer = useRef<number | undefined>(undefined);
  useEffect(() => {
    setOpen(false);
    return () => window.clearTimeout(hoverTimer.current);
  }, [disabled]);
  return (
    <BaseTooltip.Provider delay={200} closeDelay={0}>
      <BaseTooltip.Root disableHoverablePopup open={open} triggerId={id} onOpenChange={(next, details) => {
        if (details.reason === "escape-key") details.allowPropagation();
        if (!next) window.clearTimeout(hoverTimer.current);
        setOpen(next);
      }}>
        <BaseTooltip.Trigger id={id} render={cloneElement(trigger, { title: undefined })}
          onPointerEnter={disabled ? (event) => {
            if (event.pointerType === "touch") return;
            window.clearTimeout(hoverTimer.current);
            hoverTimer.current = window.setTimeout(() => setOpen(true), 200);
          } : undefined}
          onPointerLeave={disabled ? () => {
            window.clearTimeout(hoverTimer.current);
            setOpen(false);
          } : undefined} />
        <BaseTooltip.Portal>
          <BaseTooltip.Positioner
            className="ui-tooltip-positioner"
            side="right"
            align="start"
            sideOffset={8}
          >
            <BaseTooltip.Popup role="tooltip" className="ui-tooltip">
              {content}
            </BaseTooltip.Popup>
          </BaseTooltip.Positioner>
        </BaseTooltip.Portal>
      </BaseTooltip.Root>
    </BaseTooltip.Provider>
  );
}
