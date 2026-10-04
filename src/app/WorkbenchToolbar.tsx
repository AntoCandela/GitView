/** Shares workbench toolbar layout while each surface supplies its permitted controls. */

import type { ReactNode } from "react";

export function WorkbenchToolbar({ identity, selector, actions, branded = false }: {
  identity: ReactNode;
  selector: ReactNode;
  actions: ReactNode;
  branded?: boolean;
}) {
  return <header className={`workbench-toolbar${branded ? " workbench-toolbar--branded" : ""}`}>
    <div className="workbench-identity">{identity}</div>
    <div className="workspace-selectors">{selector}</div>
    <div className="workbench-layout-control">{actions}</div>
  </header>;
}
