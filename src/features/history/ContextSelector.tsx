/** Lists shared branches and routes checked-out choices to their existing worktrees. */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ContextOptionsResult } from "../../contracts/inspection";
import type { RepositoryClient } from "../../contracts/repositories";
import { BranchIcon, CheckIcon, ChevronDownIcon, WorktreeIcon } from "../../ui/icons";
import { SearchInput } from "../../ui/SearchInput";
import { Tooltip } from "../../ui/Tooltip";

type Worktree = Extract<ContextOptionsResult, { kind: "options" }>["worktrees"][number];

export function ContextSelector({ client, entryId, branch, refColors, description, onBranch, onWorktree }: {
  client: RepositoryClient;
  entryId: string;
  branch: string;
  refColors: ReadonlyMap<string, string>;
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
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss(true);
      return;
    }
    if (event.defaultPrevented) return;
    if (event.target instanceof HTMLInputElement && (event.key === "Home" || event.key === "End")) return;
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
  const checkoutByBranch = new Map<string, Worktree>();
  for (const item of options?.worktrees ?? []) {
    if (item.branch !== null && (item.current || !checkoutByBranch.has(item.branch))) checkoutByBranch.set(item.branch, item);
  }
  const currentBranch = options?.worktrees.find((item) => item.current)?.branch;
  const branchTypeOrder = (name: string) => name === currentBranch ? 0 : checkoutByBranch.has(name) ? 2 : 1;
  const branches = options?.branches.filter((item) =>
    `${item.name} ${checkoutByBranch.get(item.name)?.label ?? ""}`.toLocaleLowerCase().includes(filter))
    .sort((left, right) => branchTypeOrder(left.name) - branchTypeOrder(right.name) || left.name.localeCompare(right.name)) ?? [];
  const branchNames = new Set(options?.branches.map((item) => item.name));
  const otherWorktrees = options?.worktrees.filter((item) => (item.branch === null || !branchNames.has(item.branch)
    || checkoutByBranch.get(item.branch)?.id !== item.id)
    && `${item.label} ${item.branch ?? "Detached HEAD"}`.toLocaleLowerCase().includes(filter))
    .sort((left, right) => Number(right.current) - Number(left.current)
      || Number(left.branch === null) - Number(right.branch === null) || left.label.localeCompare(right.label)) ?? [];
  function branchChoice(name: string) {
    const checkout = checkoutByBranch.get(name);
    const target = checkout && !checkout.current ? checkout : null;
    const Icon = checkout?.current ? CheckIcon : target ? WorktreeIcon : BranchIcon;
    const action = target ? `Open worktree ${target.label} for branch ${name}` : `View branch ${name}`;
    const detail = target ? `${action}. Changes the active directory and history without checking out files.`
      : `${action} without checking out files${checkout ? ` · Current worktree: ${checkout.label}` : ""}`;
    return <Tooltip key={name} content={<ChoiceHint name={name} checkout={checkout}
      color={refColors.get(`local_branch:${name}`)} navigationAvailable={!!onWorktree} />}
      trigger={<button type="button" data-context-choice aria-description={detail}
        aria-label={action} aria-current={checkout?.current ? "true" : undefined}
        aria-pressed={branch === name} disabled={!!target && !onWorktree} style={{ color: refColors.get(`local_branch:${name}`) }}
        onClick={() => { if (target) onWorktree?.(target.id); else onBranch(name); dismiss(true); }}>
        <Icon size={14} aria-hidden="true" />
        <span className="history-context-name history-branch-name">{name}</span>
      </button>} />;
  }
  return <div className="history-context" ref={root} onKeyDown={navigate}>
    <Tooltip enabled={!open} content={description} trigger={<button type="button" className="history-context-trigger" ref={trigger}
      aria-label={`View branch or worktree: ${branch}`} aria-description={description} aria-expanded={open} aria-haspopup="dialog"
      onClick={() => setOpen(!open)}><span>{branch}</span><ChevronDownIcon size={14} aria-hidden="true" /></button>} />
    {open && <div className="history-context-disclosure" role="dialog" aria-label="Choose branch or worktree">
      <SearchInput value={query} onChange={setQuery} label="Search branches and worktrees"
        placeholder={options?.worktrees.some((item) => !item.current) ? "Search branches and worktrees…" : "Search branches…"} />
      {!result && !transportError && <p role="status">Loading choices…</p>}
      {transportError && <p role="alert">Desktop connection interrupted. Close and reopen to retry.</p>}
      {result && result.kind !== "options" && <p role="alert">{result.message}</p>}
      {options && <>
        {branches.length > 0 && <div role="group" aria-label="Repository branches">
          {branches.map((item) => branchChoice(item.name))}
        </div>}
        {otherWorktrees.length > 0 && <div role="group" aria-label="Other worktrees"
          className={branches.length > 0 ? "history-context-worktrees" : undefined}>
          {otherWorktrees.map((item) => {
            const Icon = item.current ? CheckIcon : WorktreeIcon;
            const action = item.current ? `View current worktree ${item.label}` : `Open worktree ${item.label}`;
            return <Tooltip key={item.id} content={<ChoiceHint name={item.label} checkout={item}
              color={item.branch ? refColors.get(`local_branch:${item.branch}`) : undefined} navigationAvailable={!!onWorktree} />}
              trigger={<button type="button" data-context-choice aria-label={action}
                aria-description={item.branch === null ? "Detached HEAD — no checked-out branch." : `Checked out branch: ${item.branch}`}
                aria-current={item.current ? "true" : undefined} disabled={!item.current && !onWorktree}
                onClick={() => { if (item.current) onBranch(null); else onWorktree?.(item.id); dismiss(true); }}>
                <Icon size={14} aria-hidden="true" />
                <span className="history-context-name">{item.label}</span>
                <span className="history-context-meta">{item.branch ?? "Detached HEAD"}</span>
              </button>} />;
          })}
        </div>}
        {branches.length === 0 && otherWorktrees.length === 0 && <p role="status">
          {filter ? "No matching branches or worktrees." : "No local branches."}
        </p>}
      </>}
    </div>}
  </div>;
}

function ChoiceHint({ name, checkout, color, navigationAvailable }: {
  name: string; checkout?: Worktree; color?: string; navigationAvailable: boolean;
}) {
  const navigates = checkout && !checkout.current;
  const unavailable = navigates && !navigationAvailable;
  const Icon = checkout?.current ? CheckIcon : checkout ? WorktreeIcon : BranchIcon;
  return <div className="history-context-hint">
    <div className="history-context-hint-kind">{checkout?.current ? "Current worktree"
      : checkout ? checkout.branch === null ? "Detached worktree" : "Checked out elsewhere" : "Not checked out"}</div>
    <div className="history-context-hint-title">
      <Icon size={16} aria-hidden="true" style={{ color }} />
      <strong>{name}</strong>
    </div>
    {checkout && <dl>
      <dt>Worktree</dt><dd>{checkout.label}</dd>
      {checkout.branch !== name && <><dt>Branch</dt><dd>{checkout.branch ?? "Detached HEAD — no branch"}</dd></>}
    </dl>}
    <div className="history-context-hint-action">
      <strong>{unavailable ? "Navigation unavailable" : navigates ? "Open worktree" : "View history"}</strong>
      <span>{unavailable ? "This view cannot open another worktree."
        : navigates ? "Switches working directory and history. No checkout."
        : "Working directory and files stay unchanged. No checkout."}</span>
    </div>
  </div>;
}
