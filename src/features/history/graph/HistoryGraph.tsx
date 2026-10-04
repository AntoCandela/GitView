/** Presents read-only ancestry with inline committed paths and view-only branch navigation. */

import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { defaultRangeExtractor, useVirtualizer } from "@tanstack/react-virtual";
import type { HistoryPage } from "../../../contracts/history";
import type { RepositoryClient } from "../../../contracts/repositories";
import type { CommitComparisonControls } from "../../diff";
import { RefreshIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { layoutHistory, type HistoryRow } from "./layout";
import { colorHistory } from "./colors";
import { useHistory } from "../useHistory";
import { ContextSelector } from "../ContextSelector";
import { CommitFiles } from "../CommitFiles";
import { HistoryRefs } from "./HistoryRefs";

export interface HistoryGraphProps {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  onSelectWorktree?: (worktreeId: string) => void;
  workingBranch?: string | null;
  comparison?: CommitComparisonControls;
}

const noRefs: HistoryPage["refs"] = [];
const laneSpacing = 16;

/** The parent supplies bounded flex space; expanded paths keep all outgoing lanes continuous. */
export function HistoryGraph({ client, entryId, selectionGeneration, onSelectWorktree, workingBranch, comparison }: HistoryGraphProps) {
  const context = useMemo(() => ({ client, entryId, selectionGeneration }), [client, entryId, selectionGeneration]);
  const [branchChoice, setBranchChoice] = useState<{ context: typeof context; branch: string | null } | null>(null);
  const branch = branchChoice?.context === context ? branchChoice.branch : null;
  const { page, loading, error, refresh, loadMore } = useHistory(client, entryId, selectionGeneration, branch);
  const scope = useMemo(() => ({ context, branch }), [context, branch]);
  const layout = useMemo(() => layoutHistory(page?.commits ?? []), [page?.commits]);
  const colors = useMemo(() => colorHistory(page, branch ?? page?.head.branch ?? null), [page, branch]);
  const refsByOid = useMemo(() => {
    const byOid = new Map<string, HistoryPage["refs"]>();
    for (const ref of page?.refs ?? []) {
      const existing = byOid.get(ref.commitOid);
      if (existing) existing.push(ref);
      else byOid.set(ref.commitOid, [ref]);
    }
    return byOid;
  }, [page?.refs]);
  const headOid = page?.head.state === "attached" || page?.head.state === "detached" ? page.head.oid : null;
  const headLabelId = useId();
  const headerHeights = useMemo(() => layout.rows.map(({ commit }) =>
    refsByOid.has(commit.oid) || commit.oid === headOid || commit.parents.length === 0 ? 40 : 28,
  ), [layout.rows, refsByOid, headOid]);
  const unresolvedParents = useMemo(() => [...new Map(layout.stubs.map((stub) => [stub.oid, stub])).values()], [layout.stubs]);
  const [selection, setSelection] = useState<{
    scope: typeof scope; refs: HistoryPage["refs"]; oid: string;
  } | null>(null);
  const selectedOid = selection?.scope === scope && selection.refs === page?.refs ? selection.oid : null;
  const invalidateComparison = comparison?.onInvalidate;
  useEffect(() => {
    invalidateComparison?.();
  }, [scope, selectedOid, invalidateComparison]);
  const list = useRef<HTMLDivElement>(null);
  const notices = useRef<HTMLDivElement>(null);
  const [scrollMargin, setScrollMargin] = useState(0);
  const [focus, setFocus] = useState<typeof selection>(null);
  const focusedOid = focus?.scope === scope && focus.refs === page?.refs ? focus.oid : null;
  const pendingFocus = useRef<{ index: number; last: boolean } | null>(null);
  const previousExpandedOid = useRef<string | null>(null);
  const indicesByOid = useMemo(() => new Map(layout.rows.map((row, index) => [row.commit.oid, index])), [layout]);
  const expandedIndex = selectedOid === null ? undefined : indicesByOid.get(selectedOid);
  const focusedIndex = focusedOid === null ? undefined : indicesByOid.get(focusedOid);
  const getItemKey = useCallback((index: number) => layout.rows[index].commit.oid, [layout]);
  const rangeExtractor = useCallback((range: Parameters<typeof defaultRangeExtractor>[0]) => {
    const indices = defaultRangeExtractor(range);
    // Logical endpoints preserve native Tab entry/exit; stateful rows stay mounted offscreen.
    for (const index of [0, layout.rows.length - 1, expandedIndex, focusedIndex]) {
      if (index !== undefined && index >= 0 && !indices.includes(index)) indices.push(index);
    }
    return indices.sort((first, second) => first - second);
  }, [layout.rows.length, expandedIndex, focusedIndex]);
  const virtualizer = useVirtualizer({
    count: layout.rows.length,
    getScrollElement: () => list.current,
    getItemKey,
    estimateSize: (index) => headerHeights[index],
    initialRect: { width: 480, height: 360 },
    overscan: 6,
    scrollMargin,
    rangeExtractor,
  });
  const visibleRows = virtualizer.getVirtualItems();
  const width = layout.laneCount * laneSpacing + laneSpacing;

  useLayoutEffect(() => {
    const previous = previousExpandedOid.current;
    previousExpandedOid.current = selectedOid;
    if (previous === null || previous === selectedOid) return;
    const index = indicesByOid.get(previous);
    // Offscreen expanded rows can unmount before ResizeObserver sees their collapsed size.
    if (index !== undefined) virtualizer.resizeItem(index, headerHeights[index]);
  }, [selectedOid, indicesByOid, headerHeights, virtualizer]);

  useLayoutEffect(() => {
    const element = notices.current;
    if (!element) return;
    const measure = () => setScrollMargin(element.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useLayoutEffect(() => {
    setScrollMargin(notices.current?.offsetHeight ?? 0);
  });
  useLayoutEffect(() => {
    pendingFocus.current = null;
    virtualizer.measure();
    if (list.current?.scrollTop) virtualizer.scrollToOffset(0);
  }, [scope, page?.refs, virtualizer]);
  useLayoutEffect(() => {
    if (pendingFocus.current === null) return;
    const row = list.current?.querySelector<HTMLDivElement>(`.history-row[data-index="${pendingFocus.current.index}"]`);
    const controls = row ? Array.from(row.querySelectorAll<HTMLButtonElement | HTMLSelectElement>("button:not([disabled]), select:not([disabled])"))
      .filter((control) => control.tabIndex >= 0) : [];
    const control = pendingFocus.current.last ? controls[controls.length - 1] : controls[0];
    if (control) {
      control.focus({ preventScroll: true });
      pendingFocus.current = null;
    }
  });

  function navigate(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let target: number;
    if (event.key === "ArrowDown") target = Math.min(index + 1, layout.rows.length - 1);
    else if (event.key === "ArrowUp") target = Math.max(index - 1, 0);
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = layout.rows.length - 1;
    else return;
    event.preventDefault();
    if (!page) return;
    pendingFocus.current = { index: target, last: false };
    setFocus({ scope, refs: page.refs, oid: layout.rows[target].commit.oid });
    virtualizer.scrollToIndex(target, { align: "start" });
  }

  function tabBetweenRows(event: KeyboardEvent<HTMLElement>, index: number, origin?: HTMLButtonElement) {
    if (event.key !== "Tab" || event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || !page) return;
    const row = list.current?.querySelector<HTMLDivElement>(`.history-row[data-index="${index}"]`);
    const controls = Array.from(row?.querySelectorAll<HTMLButtonElement | HTMLSelectElement>("button:not([disabled]), select:not([disabled])") ?? [])
      .filter((control) => control.tabIndex >= 0);
    const boundary = event.shiftKey ? controls[0] : controls[controls.length - 1];
    if ((origin ?? event.target) !== boundary) return;
    const target = index + (event.shiftKey ? -1 : 1);
    if (target < 0 || target >= layout.rows.length) return;
    event.preventDefault();
    pendingFocus.current = { index: target, last: event.shiftKey };
    setFocus({ scope, refs: page.refs, oid: layout.rows[target].commit.oid });
    virtualizer.scrollToIndex(target, { align: "start" });
  }

  return (
    <section className="history history-feature" aria-label="Commit ancestry" style={{ "--history-lane-width": `${width}px` } as CSSProperties}>
      <div className="history-toolbar">
        <ContextSelector key={`${entryId}:${selectionGeneration}`} client={client} entryId={entryId}
          branch={branch ?? page?.head.branch ?? (page?.head.state === "detached" ? "Detached" : "HEAD")}
          refColors={colors.refs}
          description={branch === null ? "Choose a branch. Branches checked out elsewhere open their worktrees; other branches show history without checkout."
            : `Viewing ${branch}; working files remain on ${workingBranch ?? page?.head.branch ?? "the current HEAD"}. No checkout.`}
          onBranch={(next) => {
            invalidateComparison?.(); setSelection(null);
            setBranchChoice({ context, branch: next });
          }}
          onWorktree={onSelectWorktree} />
        <Tooltip content="Refresh history" trigger={<button className="history-refresh" type="button"
          onClick={() => { invalidateComparison?.(); setSelection(null); refresh(); }} aria-label="Refresh history">
          <RefreshIcon size={16} aria-hidden="true" />
        </button>} />
      </div>
      <div className="history-scroll" ref={list} aria-busy={loading !== null}>
        <div ref={notices}>
          {loading === "initial" && <p className="history-notice" role="status">Loading history…</p>}
          {error && <p className="history-notice history-error" role="alert">{error.message}</p>}
          {page?.head.state === "unresolved" && <p className="history-notice" role="status">HEAD could not be resolved. Other available refs are shown.</p>}
          {page?.completeness === "shallow_or_missing" && <p className="history-notice" role="status">Some ancestry is shallow or unavailable.</p>}
          {page?.commits.length === 0 && <p className="history-notice" role="status">{page.head.state === "unborn" ? "No commits at HEAD." : "No reachable commits."}</p>}
        </div>
        {page && <>
          <div className="history-rows" role="group" aria-label="Commits" style={{ height: virtualizer.getTotalSize() }}>
            {visibleRows.map((virtualRow) => {
              const index = virtualRow.index;
              const row = layout.rows[index];
              const headerHeight = headerHeights[index];
              const subjectHeight = headerHeight === 28 ? 28 : 24;
              const isHead = row.commit.oid === headOid;
              const rowRefs = refsByOid.get(row.commit.oid) ?? noRefs;
              return <div className="history-row" key={virtualRow.key} data-index={index}
                ref={virtualizer.measureElement} style={{ transform: `translateY(${virtualRow.start - scrollMargin}px)` }}
                onKeyDown={(event) => tabBetweenRows(event, index)}
                onFocusCapture={() => setFocus({ scope, refs: page.refs, oid: row.commit.oid })}
                onBlurCapture={(event) => {
                  const next = event.relatedTarget;
                  const ownedPopup = next instanceof Element && next.closest("[data-history-owner]")?.getAttribute("data-history-owner") === row.commit.oid;
                  if (!event.currentTarget.contains(next) && !ownedPopup) setFocus(null);
                }}>
            <div className="history-header" data-head={isHead} data-selected={selectedOid === row.commit.oid}
              style={{ "--history-header-height": `${headerHeight}px`, "--history-subject-height": `${subjectHeight}px` } as CSSProperties}>
            <Tooltip content={<div className="history-commit-tooltip">
              <strong>{row.commit.subject ?? "Commit"}</strong>
              <code>{row.commit.oid}</code>
              <span>{selectedOid === row.commit.oid ? "Collapse" : "Expand"} changed files</span>
            </div>} trigger={<button type="button" className="history-commit" data-history-index={index}
              aria-label={`${row.commit.subject ? `${row.commit.subject}, ` : ""}Commit ${row.commit.oid}`}
              aria-describedby={isHead ? `${headLabelId}-${row.commit.oid}` : undefined}
              aria-pressed={selectedOid === row.commit.oid}
              aria-expanded={selectedOid === row.commit.oid}
              onKeyDown={(event) => navigate(event, index)}
              onClick={() => { invalidateComparison?.(); setSelection(selectedOid === row.commit.oid ? null : { scope, refs: page.refs, oid: row.commit.oid }); }}
            >
              <LaneRow row={row} width={width} height={headerHeight} nodeY={subjectHeight / 2} isHead={isHead} colors={colors.commits} />
              <span className="history-commit-content">
                <span className="history-subject">{row.commit.subject ?? row.commit.oid.slice(0, 10)}</span>
              </span>
              <code className="history-short-oid" aria-hidden="true">{row.commit.oid.slice(0, 7)}</code>
            </button>} />
              <span className="history-badges">
                {row.commit.root && row.commit.parents.length === 0 && <span className="history-badge">Root</span>}
                {!row.commit.root && row.commit.parents.length === 0 && <span className="history-badge">Ancestry unavailable</span>}
                {(rowRefs.length > 0 || isHead) && <HistoryRefs
                  refs={rowRefs} colors={colors.refs} context={scope} snapshot={page.refs}
                  commitOid={row.commit.oid} viewedBranch={branch ?? page.head.branch}
                  head={isHead ? page.head : null} headLabelId={`${headLabelId}-${row.commit.oid}`}
                  onTabOut={(event, trigger) => tabBetweenRows(event, index, trigger)} />}
              </span>
            </div>
            {selectedOid === row.commit.oid && <div className="history-expansion">
              <LaneContinuation row={row} width={width} colors={colors.commits} />
              <CommitFiles key={row.commit.oid} client={client} entryId={entryId} selectionGeneration={selectionGeneration}
                commit={row.commit} comparison={comparison} virtualScrollRef={list} />
            </div>}
            </div>;
            })}
          </div>
          {layout.stubs.length > 0 && <div className="history-boundary">
            <svg className="history-lanes" viewBox={`0 0 ${width} 20`} preserveAspectRatio="none" aria-hidden="true">
              {layout.stubs.map((stub) => <path className={`history-stub-${stub.state}`} key={stub.edgeKey} style={{ color: colors.commits.get(stub.colorOid) }} d={`M ${stub.lane * laneSpacing + laneSpacing} 0 v 16`} />)}
            </svg>
            <details className="history-stubs">
              <Tooltip content="Toggle unresolved ancestry details" trigger={<summary>Unresolved ancestry ({unresolvedParents.length})</summary>} />
              <ul>{unresolvedParents.map((stub) => <li key={stub.oid}>
                <code>{stub.oid}</code>
                <span>{stub.state === "unavailable" ? "Unavailable parent" : page.hasMore ? "Parent beyond loaded pages" : "Parent outside loaded history"}</span>
              </li>)}</ul>
            </details>
          </div>}
          {page.hasMore && <Tooltip content="Load more commits" trigger={<button className="history-load-more" type="button"
            onClick={loadMore} disabled={loading !== null || page.cursor === null}>
            {loading === "more" ? "Loading more…" : "Load more"}
          </button>} />}
        </>}
      </div>
    </section>
  );
}

function LaneRow({ row, width, height, nodeY, isHead, colors }: {
  row: HistoryRow; width: number; height: number; nodeY: number; isHead: boolean; colors: Map<string, string>;
}) {
  const nodeX = row.lane * laneSpacing + laneSpacing;
  return <svg className="history-lanes" style={{ color: colors.get(row.commit.oid) }} viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
    {row.segments.map((segment) => {
      const fromX = segment.fromLane * laneSpacing + laneSpacing;
      const toX = segment.toLane * laneSpacing + laneSpacing;
      const fromY = segment.from === "top" ? 0 : nodeY;
      const toY = segment.to === "node" ? nodeY : height;
      const direction = Math.sign(toX - fromX);
      const radius = Math.min(4, Math.abs(toX - fromX) / 2);
      let path = `M ${fromX} ${fromY} V ${toY}`;
      if (fromX !== toX) {
        // Incoming edges turn only on their actual parent's row; extra parents leave horizontally.
        path = segment.to === "node"
          ? `M ${fromX} ${fromY} V ${nodeY - radius} Q ${fromX} ${nodeY} ${fromX + direction * radius} ${nodeY} H ${toX}`
          : `M ${fromX} ${fromY} H ${toX - direction * radius} Q ${toX} ${nodeY} ${toX} ${nodeY + radius} V ${toY}`;
      }
      return <path key={`${segment.edgeKey}:${segment.from}`} style={{ color: colors.get(segment.colorOid) }} d={path} />;
    })}
    <circle className="history-node-halo" cx={nodeX} cy={nodeY} r={isHead ? 5.5 : 4.75} />
    <circle className={isHead ? "history-head-node" : "history-node"} cx={nodeX} cy={nodeY} r={isHead ? 4 : 3.5} />
  </svg>;
}

function LaneContinuation({ row, width, colors }: { row: HistoryRow; width: number; colors: Map<string, string> }) {
  return <svg className="history-lanes history-continuation" viewBox={`0 0 ${width} 1`} preserveAspectRatio="none" aria-hidden="true">
    {row.segments.filter((segment) => segment.to === "bottom").map((segment) => {
      const x = segment.toLane * laneSpacing + laneSpacing;
      return <path key={segment.edgeKey} style={{ color: colors.get(segment.colorOid) }}
        d={`M ${x} 0 V 1`} />;
    })}
  </svg>;
}
