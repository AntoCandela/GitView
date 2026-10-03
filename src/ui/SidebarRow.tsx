/** Renders a generic sidebar button with consistent visual and accessible selection. */

import type { ButtonHTMLAttributes } from "react";

export function SidebarRow({
  selected = false,
  className = "",
  ...buttonProps
}: ButtonHTMLAttributes<HTMLButtonElement> & { selected?: boolean }) {
  return (
    <button
      {...buttonProps}
      className={`sidebar-row ${selected ? "is-selected" : ""} ${className}`}
      aria-current={selected ? "true" : undefined}
    />
  );
}
