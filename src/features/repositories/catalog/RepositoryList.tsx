/**
 * Renders searchable repository rows with measured virtualization and roving focus.
 * Keyboard focus is local navigation; only activation requests a host selection.
 */

import { useCallback, useId, useLayoutEffect, useRef, useState } from "react";
import { defaultRangeExtractor, useVirtualizer } from "@tanstack/react-virtual";
import type { RepositoryEntry } from "../../../contracts/repositories";
import { FolderIcon, PencilIcon, TrashIcon } from "../../../ui/icons";
import { KebabMenu } from "../../../ui/KebabMenu";
import { Pill } from "../../../ui/Pill";
import { SidebarRow } from "../../../ui/SidebarRow";
import { Tooltip } from "../../../ui/Tooltip";
import { headLabel } from "./headLabel";
import { RepositoryNameEditor, type RepositoryRenameState } from "./RepositoryNameEditor";
import { useTranslation } from "../../../i18n";

/** Omission leaves repository rows selection-only, without management controls or drafts. */
export interface RepositoryManagement {
  onRename: (entry: RepositoryEntry) => void;
  onRemove: (id: string) => void;
  renameState: RepositoryRenameState | null;
  onRenameChange: (value: string) => void;
  onRenameSave: () => void;
  onRenameCancel: (restoreFocus?: boolean) => void;
}

export function RepositoryList({
  entries,
  emptyMessage,
  selectedId,
  onSelect,
  management,
  actionsDisabled,
  selectionDisabled = false,
}: {
  entries: readonly RepositoryEntry[];
  emptyMessage: string;
  selectedId: string | null;
  onSelect: (id: string) => void;
  management?: RepositoryManagement;
  actionsDisabled: boolean;
  selectionDisabled?: boolean;
}) {
  const { locale, t } = useTranslation();
  const renameState = management?.renameState;
  const scrollRef = useRef<HTMLElement>(null);
  const pendingFocusIndex = useRef<number | null>(null);
  const pathDescriptionId = useId();
  const [focusedIndex, setFocusedIndex] = useState(0);
  const getItemKey = useCallback(
    (index: number) => entries[index].id,
    [entries],
  );
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    getItemKey,
    estimateSize: () => 44,
    initialRect: { width: 296, height: 480 },
    overscan: 6,
    // Keep an editing row mounted when scrolling so its input and draft retain focus.
    rangeExtractor: (range) => {
      const indices = defaultRangeExtractor(range);
      const editingIndex = entries.findIndex((entry) => entry.id === renameState?.entryId);
      return editingIndex >= 0 && !indices.includes(editingIndex)
        ? [...indices, editingIndex].sort((first, second) => first - second)
        : indices;
    },
  });
  const visibleRows = virtualizer.getVirtualItems();
  // Scrolling can unmount the focused row; keep one rendered row reachable by Tab.
  const tabStop = visibleRows.some((row) => row.index === focusedIndex)
    ? focusedIndex
    : visibleRows[0]?.index;

  useLayoutEffect(() => {
    if (pendingFocusIndex.current === null) return;
    // Offscreen targets mount after scrolling; focus only once their button exists.
    const button = scrollRef.current?.querySelector<HTMLButtonElement>(
      `[data-repository-index="${pendingFocusIndex.current}"]`,
    );
    if (button) {
      button.focus();
      pendingFocusIndex.current = null;
    }
  }, [visibleRows, focusedIndex]);

  function handleKeyDown(event: React.KeyboardEvent<HTMLElement>) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
      "[data-repository-index]",
    );
    if (!button) return;
    const nextIndex =
      Number(button.dataset.repositoryIndex) +
      (event.key === "ArrowDown" ? 1 : -1);
    if (nextIndex < 0 || nextIndex >= entries.length) return;
    event.preventDefault();
    pendingFocusIndex.current = nextIndex;
    setFocusedIndex(nextIndex);
    virtualizer.scrollToIndex(nextIndex, { align: "auto" });
  }

  return (
    <nav
      className="repository-list"
      aria-label={t("repo.repositories")}
      ref={scrollRef}
      onKeyDown={handleKeyDown}
    >
      {entries.length === 0 ? (
        <p className="empty-list">{emptyMessage}</p>
      ) : null}
      <div
        className="repository-list-content"
        style={{ height: virtualizer.getTotalSize() }}
      >
        {visibleRows.map((row) => {
          const entry = entries[row.index];
          const selected = entry.id === selectedId;
          const statusLabel =
            entry.kind === "unknown"
              ? entry.availability === "unavailable"
                ? t("repo.contextUnavailable")
                : t("repo.contextChecking")
              : entry.kind === "bare"
                ? entry.availability === "unavailable"
                  ? t("repo.bareUnavailable")
                  : t("repo.bare")
                : entry.availability === "unavailable"
                  ? t("repo.unavailable")
                  : "";
          return (
            <div
              key={row.key}
              data-index={row.index}
              data-repository-id={entry.id}
              ref={virtualizer.measureElement}
              className={`repository-virtual-row${selected ? " is-selected" : ""}`}
              style={{ transform: `translateY(${row.start}px)` }}
            >
              {management && renameState?.entryId === entry.id ? (
                <RepositoryNameEditor
                  key={entry.id}
                  entry={entry}
                  state={renameState}
                  selected={selected}
                  pathDescriptionId={`${pathDescriptionId}-${row.index}`}
                  onChange={management.onRenameChange}
                  onSave={management.onRenameSave}
                  onCancel={management.onRenameCancel}
                />
              ) : (
              <Tooltip
                content={entry.locationLabel}
                trigger={
                  <SidebarRow
                    className="repository-row"
                    selected={selected}
                    type="button"
                    data-repository-index={row.index}
                    disabled={selectionDisabled}
                    onClick={() => { if (!selectionDisabled) onSelect(entry.id); }}
                    onFocus={() => setFocusedIndex(row.index)}
                    tabIndex={row.index === tabStop ? 0 : -1}
                    aria-describedby={`${pathDescriptionId}-${row.index}`}
                  >
                    <span className="repository-summary">
                      <span className="repository-identity">
                        <FolderIcon aria-hidden="true" />
                        <span className="repository-name">
                          {entry.repositoryLabel}
                        </span>
                      </span>
                      <Pill>{headLabel(entry, locale)}</Pill>
                    </span>
                    {statusLabel ? (
                      <span className="repository-meta">{statusLabel}</span>
                    ) : null}
                  </SidebarRow>
                }
              />
              )}
              {management ? <KebabMenu
                label={t("repo.actions", { name: entry.repositoryLabel })}
                disabled={actionsDisabled || renameState?.entryId === entry.id}
                items={[
                  {
                    label: t("repo.rename"),
                    icon: <PencilIcon aria-hidden="true" />,
                    onSelect: () => management.onRename(entry),
                  },
                  {
                    label: t("repo.remove"),
                    icon: <TrashIcon aria-hidden="true" />,
                    destructive: true,
                    onSelect: () => management.onRemove(entry.id),
                  },
                ]}
              /> : null}
              {/* Descriptions must remain available even when the tooltip popup is closed. */}
              <span hidden id={`${pathDescriptionId}-${row.index}`}>
                {entry.locationLabel}
              </span>
            </div>
          );
        })}
      </div>
    </nav>
  );
}
