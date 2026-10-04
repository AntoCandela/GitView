/** Presents the six controlled panel arrangements without owning workbench or repository state. */

import { useRef } from "react";
import { Popover } from "@base-ui/react/popover";
import { LayoutIcon } from "../../ui/icons";
import { RadioGroup } from "../../ui/RadioGroup";
import { Tooltip } from "../../ui/Tooltip";
import { workbenchLayouts, type WorkbenchLayoutId, type WorkbenchPanel } from "./workbenchLayout";
import { useTranslation } from "../../i18n";


export interface WorkbenchLayoutMenuProps {
  value: WorkbenchLayoutId;
  onChange: (value: WorkbenchLayoutId) => void;
}

export function WorkbenchLayoutMenu({ value, onChange }: WorkbenchLayoutMenuProps) {
  const popup = useRef<HTMLDivElement>(null);
  const { t } = useTranslation();
  const panelLabels: Record<WorkbenchPanel, string> = {
    comparison: t("app.panel.preview"), files: t("app.panel.files"), history: t("app.panel.graph"),
  };
  const layoutOptions = workbenchLayouts.map((layout) => ({
    value: layout.id,
    label: t("app.panel.arrangement", { top: panelLabels[layout.top], left: panelLabels[layout.left], right: panelLabels[layout.right] }),
    content: <span className="workbench-layout-diagram" aria-hidden="true">
      <span className={`workbench-layout-slot is-top is-${layout.top}`}>{panelLabels[layout.top]}</span>
      <span className={`workbench-layout-slot is-${layout.left}`}>{panelLabels[layout.left]}</span>
      <span className={`workbench-layout-slot is-${layout.right}`}>{panelLabels[layout.right]}</span>
    </span>,
  }));

  return (
    <Popover.Root>
      <Tooltip content={t("app.chooseLayout")} trigger={<Popover.Trigger className="ui-kebab-trigger" aria-label={t("app.layout")}>
        <LayoutIcon aria-hidden="true" />
      </Popover.Trigger>} />
      <Popover.Portal>
        <Popover.Positioner
          className="workbench-layout-positioner"
          side="bottom"
          align="end"
          sideOffset={4}
          collisionPadding={8}
        >
          <Popover.Popup
            ref={popup}
            className="workbench-layout-popup"
            initialFocus={() => popup.current?.querySelector<HTMLInputElement>('input:checked') ?? true}
          >
            <Popover.Title className="workbench-layout-title">{t("app.layout")}</Popover.Title>
            <RadioGroup
              className="workbench-layout-options"
              label={t("app.panelArrangement")}
              value={value}
              options={layoutOptions}
              onChange={onChange}
            />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
