/** Summarizes commit references and reveals grouped exact names without changing selection or Git state. */
import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Popover } from "@base-ui/react/popover";
import type { HistoryPage } from "../../../contracts/history";
import { BranchIcon, RemoteBranchIcon, TagIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { formatNumber, useTranslation, type MessageKey } from "../../../i18n";

type Reference = HistoryPage["refs"][number];
const kindKeys = {
  local_branch: "history.ref.localBranch", remote_tracking: "history.ref.remoteTracking", tag: "history.ref.tag",
} as const satisfies Record<Reference["kind"], MessageKey>;
const groups = [
  { kind: "local_branch", label: "history.refs.localBranches", more: "history.refs.moreLocalBranches", Icon: BranchIcon },
  { kind: "remote_tracking", label: "history.refs.remoteBranches", more: "history.refs.moreRemoteBranches", Icon: RemoteBranchIcon },
  { kind: "tag", label: "history.refs.tags", more: "history.refs.moreTags", Icon: TagIcon },
] as const;

/** Snapshot identity closes stale disclosures; the summary never hides references from the full inventory. */
export function HistoryRefs({ refs, colors, context, snapshot, commitOid, viewedBranch, head, headLabelId, onTabOut }: {
  refs: HistoryPage["refs"];
  colors: ReadonlyMap<string, string>;
  context: object;
  snapshot: HistoryPage["refs"];
  commitOid: string;
  viewedBranch: string | null;
  head: HistoryPage["head"] | null;
  headLabelId: string;
  onTabOut: (event: KeyboardEvent<HTMLElement>, trigger: HTMLButtonElement) => void;
}) {
  const { locale, t } = useTranslation();
  const activeTrigger = useRef<HTMLButtonElement | null>(null);
  const restoreFocus = useRef(false);
  const id = useId();
  const [choice, setChoice] = useState<{ context: object; snapshot: typeof snapshot; key: string } | null>(null);
  const [dismissedHint, setDismissedHint] = useState<string | null>(null);
  const openKey = choice?.context === context && choice.snapshot === snapshot ? choice.key : null;
  const primary = primaryReference(refs, viewedBranch, head);
  const combinedHead = head?.state === "attached" && primary?.kind === "local_branch" && primary.name === head.branch;
  const headLabel = head ? head.scope === "repository"
    ? t(head.state === "detached" ? "history.refs.repositoryDetachedHead" : "history.refs.repositoryHead")
    : head.state === "detached" ? t("history.detachedHead") : "HEAD" : null;
  const hidden = refs.length - (primary ? 1 : 0);
  const details = <ReferenceDetails refs={refs} colors={colors} head={head} />;

  function disclosure(key: string, label: string, triggerContent: ReactNode, current = false) {
    const triggerId = `${id}-${key}`;
    return <Popover.Root key={key} open={openKey === key} onOpenChange={(open, event) => {
      restoreFocus.current = event.reason === "escape-key";
      if (event.reason === "escape-key") setDismissedHint(key);
      if (open) activeTrigger.current = document.getElementById(triggerId) as HTMLButtonElement | null;
      setChoice(open ? { context, snapshot, key } : null);
    }}>
      <Tooltip enabled={openKey === null && dismissedHint !== key} content={<div className="history-ref-hover">
        <div className="history-ref-title">{t("history.refs.title")}</div>
        <ReferenceDetails refs={refs} colors={colors} head={head} preview />
        <div className="history-ref-hint">{t("history.refs.keepOpen")}</div>
      </div>} trigger={<Popover.Trigger id={triggerId}
        className={`history-badge history-ref${key === "overflow" ? " history-ref-overflow" : " history-ref-primary"}`}
        data-current={current} aria-label={label}
        onPointerEnter={() => setDismissedHint(null)}
        onBlur={() => setDismissedHint(null)}>{triggerContent}</Popover.Trigger>} />
      <Popover.Portal>
        <Popover.Positioner className="history-ref-positioner" side="bottom" align="start" sideOffset={4} collisionPadding={8}>
          <Popover.Popup className="history-ref-popup" data-history-owner={commitOid}
            initialFocus finalFocus={() => restoreFocus.current ? activeTrigger.current : false}
            onKeyDown={(event) => {
              if (event.key !== "Tab" || event.altKey || event.ctrlKey || event.metaKey) return;
              const trigger = activeTrigger.current;
              if (!trigger) return;
              restoreFocus.current = false;
              trigger.focus({ preventScroll: true });
              setChoice(null);
              onTabOut(event, trigger);
              event.stopPropagation();
            }}>
            <Popover.Title className="history-ref-title">{t("history.refs.title")}</Popover.Title>
            {details}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>;
  }

  const Icon = primary ? groups.find((group) => group.kind === primary.kind)!.Icon : BranchIcon;
  return <span className="history-refs">
    {headLabel && !combinedHead && <span id={headLabelId} className="history-badge history-badge-head">{headLabel}</span>}
    {primary && disclosure(`${primary.kind}:${primary.name}`, combinedHead
      ? t("history.refs.currentReference", { reference: t(kindKeys[primary.kind], { name: primary.name }), head: headLabel! })
      : t(kindKeys[primary.kind], { name: primary.name }), <>
      {combinedHead && <span id={headLabelId} className="history-ref-head">{headLabel}<span aria-hidden="true"> · </span></span>}
      <Icon size={11} aria-hidden="true" style={{ color: colors.get(`${primary.kind}:${primary.name}`) }} />
      <ReferenceName reference={primary} />
    </>, combinedHead || primary.kind === "local_branch" && primary.name === viewedBranch)}
    {hidden > 0 && disclosure("overflow", t("history.refs.showMore", { count: hidden }), `+${formatNumber(locale, hidden)}`)}
  </span>;
}

function primaryReference(refs: HistoryPage["refs"], viewedBranch: string | null, head: HistoryPage["head"] | null) {
  const local = (name: string | null | undefined) => refs.find((ref) => ref.kind === "local_branch" && ref.name === name);
  return (head?.state === "attached" ? local(head.branch) : undefined)
    ?? local(viewedBranch)
    ?? local("main")
    ?? refs.find((ref) => ref.kind === "local_branch")
    ?? refs.find((ref) => ref.kind === "remote_tracking")
    ?? refs[0];
}

function ReferenceDetails({ refs, colors, head, preview = false }: {
  refs: HistoryPage["refs"]; colors: ReadonlyMap<string, string>; head: HistoryPage["head"] | null; preview?: boolean;
}) {
  const { locale, t } = useTranslation();
  return <div className="history-ref-details">
    {head && <p className="history-ref-context">{t(head.scope === "repository"
      ? "history.refs.repositoryContext" : "history.refs.worktreeContext", {
      state: head.state, branch: head.branch ?? "",
    })}</p>}
    {groups.map(({ kind, label, more, Icon }) => {
      const entries = refs.filter((ref) => ref.kind === kind);
      if (entries.length === 0) return null;
      const shown = preview ? entries.slice(0, 4) : entries;
      return <section className="history-ref-group" key={kind} aria-label={t(label)}>
        <h3>{t(label)} <span>{formatNumber(locale, entries.length)}</span></h3>
        <ul>{shown.map((ref) => <li key={ref.name}>
          <Icon size={12} aria-hidden="true" style={{ color: colors.get(`${ref.kind}:${ref.name}`) }} />
          <span className={`history-ref-full-name${ref.kind === "tag" ? "" : " history-branch-name"}`}>{ref.name}</span>
        </li>)}</ul>
        {shown.length < entries.length && <div className="history-ref-hint">{t(more, { count: entries.length - shown.length })}</div>}
      </section>;
    })}
  </div>;
}

/** The invisible full name preserves intrinsic width so truncation cannot oscillate when resized. */
function ReferenceName({ reference }: { reference: Reference }) {
  const label = useRef<HTMLSpanElement>(null);
  const measure = useRef<HTMLSpanElement>(null);
  const [truncated, setTruncated] = useState(false);
  useLayoutEffect(() => {
    if (!label.current || !measure.current) return;
    const update = () => setTruncated(measure.current!.getBoundingClientRect().width > label.current!.getBoundingClientRect().width + 0.5);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(label.current);
    observer.observe(measure.current);
    return () => observer.disconnect();
  }, [reference.name]);
  const remoteEnd = reference.kind === "remote_tracking" ? reference.name.indexOf("/") + 1 : 0;
  const name = Array.from(reference.name.slice(remoteEnd));
  const split = Math.ceil(name.length / 2);
  return <span className={`history-ref-name${reference.kind === "tag" ? "" : " history-branch-name"}`} ref={label} aria-hidden="true">
    <span className="history-ref-name-measure" ref={measure}>{reference.name}</span>
    <span className="history-ref-name-display">{truncated ? <>
      {remoteEnd > 0 && <span className="history-ref-remote">{reference.name.slice(0, remoteEnd)}</span>}
      <span className="history-ref-leading">{name.slice(0, split).join("")}</span>
      <span className="history-ref-trailing"><span>{name.slice(split).join("")}</span></span>
    </> : reference.name}</span>
  </span>;
}
