/** Filters repository-wide branch choices by checkout location, separately from view and navigation actions. */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ContextOptionsResult } from "../../contracts/inspection";
import type { RepositoryClient } from "../../contracts/repositories";
import { ChevronDownIcon, OpenWorktreeIcon, WorktreeIcon } from "../../ui/icons";
import { SearchInput } from "../../ui/SearchInput";
import { Tooltip } from "../../ui/Tooltip";

export function ContextSelector({ client, entryId, branch, description, onBranch, onWorktree }: {
  client: RepositoryClient;
  entryId: string;
  branch: string;
  onBranch: (branch: string | null) => void;
  description: string;
  onWorktree?: (worktreeId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [state, setState] = useState<{
    client: RepositoryClient; entryId: string; result: ContextOptionsResult | null; transportError: boolean; worktreeId: string | null;
  } | null>(null);
  const currentState = state?.client === client && state.entryId === entryId ? state : null;
  const result = currentState?.result ?? null;
  const transportError = currentState?.transportError ?? false;
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  function dismiss(restoreFocus = false) {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus();
  }
  useEffect(() => {
    if (!open) return;
    let current = true;
    setState(null);
    setQuery("");
    void client.listContexts(entryId).then((next) => {
      if (current) setState({ client, entryId, result: next, transportError: false, worktreeId: null });
    }).catch(() => { if (current) setState({ client, entryId, result: null, transportError: true, worktreeId: null }); });
    root.current?.querySelector<HTMLInputElement>('input[type="search"]')?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) dismiss();
    };
    document.addEventListener("pointerdown", outside);
    return () => { current = false; document.removeEventListener("pointerdown", outside); };
  }, [open, client, entryId]);

  function navigate(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss(true);
      return;
    }
    if (event.defaultPrevented) return;
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || !open) return;
    const choices = Array.from(root.current?.querySelectorAll<HTMLButtonElement>("[data-context-choice]:not(:disabled)") ?? []);
    if (choices.length === 0) return;
    const index = choices.indexOf(document.activeElement as HTMLButtonElement);
    const target = event.key === "Home" ? 0 : event.key === "End" ? choices.length - 1
      : event.key === "ArrowDown" ? Math.min(index + 1, choices.length - 1) : Math.max(index - 1, 0);
    event.preventDefault();
    choices[target]?.focus();
  }
  const filter = query.trim().toLocaleLowerCase();
  const options = result?.kind === "options" ? result : null;
  const selectedWorktree = options?.worktrees.find((item) => item.id === currentState?.worktreeId);
  const scopedBranches = options?.branches.filter((item) => !selectedWorktree || item.name === selectedWorktree.branch) ?? [];
  const branches = scopedBranches.filter((item) => item.name.toLocaleLowerCase().includes(filter));
  const worktrees = options?.worktrees.filter((item) => `${item.label} ${item.branch ?? ""}`.toLocaleLowerCase().includes(filter)) ?? [];
  const checkoutLabels = new Map<string, string[]>();
  for (const item of options?.worktrees ?? []) {
    if (item.branch === null) continue;
    const labels = checkoutLabels.get(item.branch);
    if (labels) labels.push(item.label);
    else checkoutLabels.set(item.branch, [item.label]);
  }
  function filterWorktree(worktreeId: string | null) {
    setState((previous) => previous?.client === client && previous.entryId === entryId ? { ...previous, worktreeId } : previous);
  }
  return <div className="history-context" ref={root} onKeyDown={navigate}>
    <Tooltip content={description} trigger={<button type="button" className="history-context-trigger" ref={trigger}
      aria-label={`View branch or worktree: ${branch}`} aria-description={description} aria-expanded={open} aria-haspopup="dialog"
      onClick={() => setOpen(!open)}><span>{branch}</span><ChevronDownIcon size={14} aria-hidden="true" /></button>} />
    {open && <div className="history-context-disclosure" role="dialog" aria-label="Choose branch or worktree">
      <SearchInput value={query} onChange={setQuery} label="Search branches and worktrees" placeholder="Search branches and worktrees…" />
      {!result && !transportError && <p role="status">Loading choices…</p>}
      {transportError && <p role="alert">Desktop connection interrupted. Close and reopen to retry.</p>}
      {result && result.kind !== "options" && <p role="alert">{result.message}</p>}
      {result?.kind === "options" && <>
        <div role="group" aria-label="Worktrees">
          <h3>Worktrees <span>Filter branches</span></h3>
          <Tooltip content="Show all repository branches, including branches not checked out in a worktree"
            trigger={<button type="button" data-context-choice aria-pressed={!selectedWorktree}
              onClick={() => filterWorktree(null)}>All worktrees</button>} />
          {worktrees.map((item) => <div className="history-worktree-choice" key={item.id}>
            <Tooltip content={`Filter branches by ${item.label} · ${item.branch ?? "Detached HEAD"}${item.current ? " · Current worktree" : ""}. Does not open the worktree.`}
              trigger={<button type="button" data-context-choice aria-label={`Filter branches by worktree ${item.label}`}
                aria-pressed={selectedWorktree?.id === item.id} onClick={() => filterWorktree(item.id)}>
                <WorktreeIcon size={14} aria-hidden="true" />
                <span className="history-context-name">{item.label}</span>
                <span className="history-context-meta">{item.branch ?? "Detached HEAD"}</span>
                {item.current && <span className="history-context-current">Current</span>}
              </button>} />
            <Tooltip content={`Open worktree ${item.label} · ${item.branch ?? "Detached HEAD"}${item.current ? " · Current worktree" : ""}`}
              trigger={<button type="button" data-context-choice className="history-worktree-open"
                disabled={!onWorktree} aria-label={`Open worktree ${item.label}`} aria-current={item.current ? "true" : undefined}
                onClick={() => { onWorktree?.(item.id); dismiss(true); }}>
                <OpenWorktreeIcon size={14} aria-hidden="true" />
              </button>} />
          </div>)}
          {worktrees.length === 0 && <p>{filter ? "No matching worktrees." : "No worktrees available."}</p>}
        </div>
        <div role="group" aria-label="Repository branches">
          <h3>Repository branches <span>View only</span></h3>
          {selectedWorktree && <p className="history-context-scope">Filtered to {selectedWorktree.label}</p>}
          {branches.map((item) => {
            const labels = checkoutLabels.get(item.name)?.join(", ");
            const checkout = labels ? `Checked out in ${labels}` : undefined;
            return <Tooltip key={item.name}
              content={`View branch ${item.name} without checking out files${checkout ? ` · ${checkout}` : ""}`}
              trigger={<button type="button" data-context-choice aria-description={checkout}
                aria-label={`View branch ${item.name}`} aria-pressed={branch === item.name}
                onClick={() => { onBranch(item.name); dismiss(true); }}>
                <span className="history-context-name">{item.name}</span>
                {labels && <span className="history-context-checkout" aria-hidden="true">
                  <WorktreeIcon size={14} /><span>{labels}</span>
                </span>}
              </button>} />;
          })}
          {branches.length === 0 && <p role="status">{selectedWorktree?.branch === null
            ? "Detached HEAD — no checked-out branch."
            : selectedWorktree && scopedBranches.length === 0 ? "No local branch available for this worktree."
              : filter ? "No matching branches." : "No local branches."}</p>}
        </div>
      </>}
    </div>}
  </div>;
}
