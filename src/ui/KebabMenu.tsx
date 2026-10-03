/** Provides a portaled action menu whose interactions cannot activate an enclosing row. */

import type { ReactNode, SyntheticEvent } from "react";
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { EllipsisIcon } from "./icons";
import { Tooltip } from "./Tooltip";

export interface KebabMenuItem {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  destructive?: boolean;
  disabled?: boolean;
}

export interface KebabMenuProps {
  label: string;
  items: readonly KebabMenuItem[];
  disabled?: boolean;
}

function stopMenuPropagation(event: SyntheticEvent) {
  // Leave default handling intact so Base UI still owns navigation and dismissal.
  event.stopPropagation();
}

/** Render beside, not inside, a row button; label names the action trigger and menu. */
export function KebabMenu({ label, items, disabled }: KebabMenuProps) {
  return (
    <BaseMenu.Root disabled={disabled}>
      <Tooltip content={label} trigger={<BaseMenu.Trigger
        className="ui-kebab-trigger"
        aria-label={label}
        disabled={disabled}
        onClick={stopMenuPropagation}
        onPointerDown={stopMenuPropagation}
        onPointerUp={stopMenuPropagation}
        onKeyDown={stopMenuPropagation}
        onKeyUp={stopMenuPropagation}
      >
        <EllipsisIcon aria-hidden="true" />
      </BaseMenu.Trigger>} />
      <BaseMenu.Portal
        onClick={stopMenuPropagation}
        onPointerDown={stopMenuPropagation}
        onPointerUp={stopMenuPropagation}
        onKeyDown={stopMenuPropagation}
        onKeyUp={stopMenuPropagation}
      >
        <BaseMenu.Positioner
          className="ui-menu-positioner"
          side="bottom"
          align="end"
          sideOffset={4}
        >
          <BaseMenu.Popup
            className="ui-menu"
            aria-label={label}
            onClick={stopMenuPropagation}
            onPointerDown={stopMenuPropagation}
            onPointerUp={stopMenuPropagation}
            onKeyDown={stopMenuPropagation}
            onKeyUp={stopMenuPropagation}
          >
            {items.map((item, index) => (
              <Tooltip key={index} content={item.label} trigger={<BaseMenu.Item
                className={`ui-menu-item${item.destructive ? " is-destructive" : ""}`}
                label={item.label}
                disabled={item.disabled}
                onClick={item.onSelect}
              >
                {item.icon && (
                  <span className="ui-menu-item-icon" aria-hidden="true">
                    {item.icon}
                  </span>
                )}
                <span>{item.label}</span>
              </BaseMenu.Item>} />
            ))}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}
