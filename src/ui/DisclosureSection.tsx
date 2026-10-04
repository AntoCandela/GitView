/** Groups dense controls behind a native keyboard-accessible disclosure with one shared chevron. */
import type { ReactNode } from "react";
import { ChevronRightIcon } from "./icons";
import { Tooltip } from "./Tooltip";
import { useTranslation } from "../i18n";

export function DisclosureSection({ title, detail, children }: {
  title: string;
  detail?: string;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return <details className="ui-disclosure">
    <Tooltip content={t("common.toggleSection", { title })} trigger={
      <summary><ChevronRightIcon aria-hidden="true" /><span>{title}</span>{detail ? <small>{detail}</small> : null}</summary>
    } />
    <div className="ui-disclosure-content">{children}</div>
  </details>;
}
