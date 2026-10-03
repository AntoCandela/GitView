/** Provides searchable view-only branch choices and navigation to existing native worktrees. */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ContextOptionsResult } from "../../contracts/inspection";
import type { RepositoryClient } from "../../contracts/repositories";
import { ChevronDownIcon } from "../../ui/icons";
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
    client: RepositoryClient; entryId: string; result: ContextOptionsResult | null; transportError: boolean;
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
      if (current) setState({ client, entryId, result: next, transportError: false });
    }).catch(() => { if (current) setState({ client, entryId, result: null, transportError: true }); });
    root.current?.querySelector<HTMLInputElement>('input[type="search"]')?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) dismiss();
    };
    document.addEventListener("pointerdown", outside);
    return () => { current = false; document.removeEventListener("pointerdown", outside); };
  }, [open, client, entryId]);

  function navigate(event: KeyboardEvent<HTMLDivElement>) {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss(true);
      return;
    }
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
  const branches = result?.kind === "options" ? result.branches.filter((item) => item.name.toLocaleLowerCase().includes(filter)) : [];
  const worktrees = result?.kind === "options" ? result.worktrees.filter((item) => `${item.label} ${item.branch ?? ""}`.toLocaleLowerCase().includes(filter)) : [];
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
        <div role="group" aria-label="Local branches">
          <h3>Local branches <span>View only</span></h3>
          {branches.map((item) => <Tooltip key={item.name} content={`View branch ${item.name} without checking out files`}
            trigger={<button type="button" data-context-choice
              aria-label={`View branch ${item.name}`} aria-pressed={branch === item.name}
              onClick={() => { onBranch(item.name); dismiss(true); }}>{item.name}</button>} />)}
          {branches.length === 0 && <p>No matching local branches.</p>}
        </div>
        <div role="group" aria-label="Worktrees">
          <h3>Worktrees</h3>
          {worktrees.map((item) => <Tooltip key={item.id} content={`Open worktree ${item.label} · ${item.branch ?? "Detached"}${item.current ? " · Current" : ""}`}
            trigger={<button type="button" data-context-choice
              disabled={!onWorktree} aria-label={`Open worktree ${item.label}`} aria-current={item.current ? "true" : undefined}
              onClick={() => { onWorktree?.(item.id); dismiss(true); }}>
              <span>{item.label}</span><span className="history-context-meta">{item.branch ?? "Detached"}{item.current ? " · Current" : ""}</span>
            </button>} />)}
          {worktrees.length === 0 && <p>No matching worktrees.</p>}
        </div>
      </>}
    </div>}
  </div>;
}
