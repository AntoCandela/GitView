/** Edits a repository's app display name in its row without activating the repository. */

import { useEffect, useId, useRef } from "react";
import type { RepositoryEntry } from "../../../contracts/repositories";
import { FolderIcon } from "../../../ui/icons";
import { Pill } from "../../../ui/Pill";
import { headLabel } from "./headLabel";

/** Workspace-owned drafts survive virtualized rows; only native replies commit names. */
export interface RepositoryRenameState {
  entryId: string;
  displayName: string;
  busy: boolean;
  error: string | null;
}

export function RepositoryNameEditor({ entry, state, selected, pathDescriptionId, onChange, onSave, onCancel }: {
  entry: RepositoryEntry;
  state: RepositoryRenameState;
  selected: boolean;
  pathDescriptionId: string;
  onChange: (value: string) => void;
  onSave: () => void;
  onCancel: (restoreFocus?: boolean) => void;
}) {
  const inputId = useId();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (state.busy) return;
    // The menu restores its own trigger first; move into the editor after it closes.
    const frame = requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
    return () => cancelAnimationFrame(frame);
  }, [state.busy]);

  return (
    <form
      className={`repository-row repository-name-editor${selected ? " is-selected" : ""}`}
      aria-label={`Rename ${entry.repositoryLabel}`}
      aria-busy={state.busy}
      onClick={(event) => event.stopPropagation()}
      onSubmit={(event) => { event.preventDefault(); event.stopPropagation(); onSave(); }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Escape") { event.preventDefault(); onCancel(); }
      }}
      onBlur={(event) => {
        if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget))
          onCancel(false);
      }}
    >
      <div className="repository-summary">
        <span className="repository-identity">
          <FolderIcon aria-hidden="true" />
          <input
            id={inputId}
            ref={inputRef}
            aria-label="Display name"
            aria-describedby={state.error ? `${pathDescriptionId} ${inputId}-error` : pathDescriptionId}
            aria-invalid={state.error !== null || !state.displayName.trim()}
            value={state.displayName}
            onChange={(event) => onChange(event.target.value)}
            readOnly={state.busy}
            required
          />
        </span>
        <Pill>{headLabel(entry)}</Pill>
      </div>
      {state.error ? <p id={`${inputId}-error`} role="alert">{state.error}</p> : null}
    </form>
  );
}
