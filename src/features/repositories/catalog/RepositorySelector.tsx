/** Shares repository disclosure chrome and dismissal without owning selection or management authority. */

import { useEffect, useId, useRef, type ReactNode, type RefObject } from "react";
import type { RepositoryEntry } from "../../../contracts/repositories";
import { useTranslation } from "../../../i18n";
import { ChevronDownIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";

export function RepositorySelector({ active, open, onOpenChange, children, disabled = false, controlsRef }: {
  active: RepositoryEntry | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
  disabled?: boolean;
  controlsRef?: RefObject<HTMLDivElement | null>;
}) {
  const { t } = useTranslation();
  const localControlsRef = useRef<HTMLDivElement>(null);
  const rootRef = controlsRef ?? localControlsRef;
  const disclosureId = useId();

  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) onOpenChange(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [open, onOpenChange, rootRef]);

  return <div className="repository-controls" ref={rootRef} onKeyDown={(event) => {
    if (event.key !== "Escape" || event.defaultPrevented || !open) return;
    // Nested editors and menus consume their own Escape before it reaches this disclosure.
    event.preventDefault();
    event.stopPropagation();
    onOpenChange(false);
    rootRef.current?.querySelector<HTMLButtonElement>(".repository-selector")?.focus();
  }}>
    <Tooltip content={active?.locationLabel ?? t("app.chooseRepository")} trigger={
      <button type="button" className="repository-selector"
        aria-label={active ? t("app.currentRepository", { name: active.repositoryLabel }) : t("app.noCurrentRepository")}
        aria-expanded={open} aria-controls={disclosureId} disabled={disabled}
        onClick={() => { if (!disabled) onOpenChange(!open); }}>
        <span>{active?.repositoryLabel ?? t("app.repositories")}</span>
        <ChevronDownIcon aria-hidden="true" />
      </button>
    } />
    {open ? <div className="repository-disclosure" id={disclosureId}>{children}</div> : null}
  </div>;
}
