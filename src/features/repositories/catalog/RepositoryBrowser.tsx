/** Reuses repository admission, search and management rows in the selector and workspace sidebar. */

import type { ComponentProps } from "react";
import { SearchInput } from "../../../ui/SearchInput";
import { PlusIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { RepositoryList } from "./RepositoryList";

export function RepositoryBrowser({ query, onQueryChange, opening, onOpen, list }: {
  query: string;
  onQueryChange: (query: string) => void;
  opening: boolean;
  onOpen: () => void;
  list: ComponentProps<typeof RepositoryList>;
}) {
  return <>
    <div className="repository-toolbar">
      <SearchInput value={query} onChange={onQueryChange} label="Search repositories" placeholder="Search repositories…" />
      <div className="repository-toolbar-actions">
        <Tooltip content={opening ? "Choosing a repository folder" : "Open repository"} trigger={
          <button className="open-repository" type="button" onClick={onOpen} disabled={opening}>
            <PlusIcon aria-hidden="true" />{opening ? "Choosing folder…" : "Open repository"}
          </button>
        } />
      </div>
    </div>
    <RepositoryList key={query.trim().toLocaleLowerCase()} {...list} />
  </>;
}
