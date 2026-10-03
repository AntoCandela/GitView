/** Renders compact metadata using the shared badge style. */

import type { ReactNode } from "react";

export function Pill({ children }: { children: ReactNode }) {
  return <span className="ui-pill">{children}</span>;
}
