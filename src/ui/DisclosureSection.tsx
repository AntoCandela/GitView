/** Groups dense controls behind a native keyboard-accessible disclosure with one shared chevron. */
import type { ReactNode } from "react";
import { ChevronRightIcon } from "./icons";
import { Tooltip } from "./Tooltip";

export function DisclosureSection({ title, detail, children }: {
  title: string;
  detail?: string;
  children: ReactNode;
}) {
  return <details className="ui-disclosure">
    <Tooltip content={`Toggle ${title}`} trigger={
      <summary><ChevronRightIcon aria-hidden="true" /><span>{title}</span>{detail ? <small>{detail}</small> : null}</summary>
    } />
    <div className="ui-disclosure-content">{children}</div>
  </details>;
}
