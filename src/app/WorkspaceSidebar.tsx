/** Keeps the repository file explorer mounted while its workspace sidebar is collapsed. */

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ResizeDivider } from "../ui/resize/ResizeDivider";
import { usePanelLayout } from "../ui/resize/panelLayout";
import { constrainResize } from "../ui/resize/resizeGeometry";
import { useTranslation } from "../i18n";

export function WorkspaceSidebar({ open, children }: { open: boolean; children: ReactNode }) {
  const { t } = useTranslation();
  const sidebarRef = useRef<HTMLElement>(null);
  const bodyRef = useRef<HTMLElement>(null);
  const [preferredWidth, setPreferredWidth] = useState(280);
  const [bounds, setBounds] = useState({ width: window.innerWidth, overlay: window.innerWidth <= 720 });
  const { resetVersion } = usePanelLayout();

  useLayoutEffect(() => {
    const body = sidebarRef.current?.parentElement;
    if (!body) return;
    bodyRef.current = body;
    const measure = () => {
      const overlay = window.innerWidth <= 720;
      const measuredWidth = body.getBoundingClientRect().width;
      const width = overlay ? Math.min(measuredWidth, window.innerWidth) : measuredWidth;
      setBounds((current) => current.width === width && current.overlay === overlay ? current : { width, overlay });
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(body);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
      bodyRef.current = null;
    };
  }, []);

  useLayoutEffect(() => setPreferredWidth(280), [resetVersion]);

  const maximum = Math.max(0, Math.min(480, bounds.width - (bounds.overlay ? 40 : 320)));
  const minimum = Math.min(180, maximum);
  const width = constrainResize(preferredWidth, minimum, maximum);
  return <aside ref={sidebarRef} id="workspace-sidebar" className="workspace-sidebar" aria-label={t("app.sidebar")}
    hidden={!open} style={{ width, flexBasis: width }}>
    {children}
    <ResizeDivider label={t("app.resizeSidebar")} controls="workspace-sidebar" root={bodyRef}
      value={width} min={minimum} max={maximum} onChange={setPreferredWidth} className="workspace-sidebar-divider" />
  </aside>;
}
