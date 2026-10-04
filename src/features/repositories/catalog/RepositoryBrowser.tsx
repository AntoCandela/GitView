/** Reuses repository admission, search and management rows in the selector and workspace sidebar. */

import type { ComponentProps } from "react";
import { SearchInput } from "../../../ui/SearchInput";
import { PlusIcon } from "../../../ui/icons";
import { Tooltip } from "../../../ui/Tooltip";
import { RepositoryList } from "./RepositoryList";
import { useTranslation } from "../../../i18n";

export function RepositoryBrowser({ query, onQueryChange, opening, onOpen, list }: {
  query: string;
  onQueryChange: (query: string) => void;
  opening: boolean;
  onOpen: () => void;
  list: ComponentProps<typeof RepositoryList>;
}) {
  const { t } = useTranslation();
  return <>
    <div className="repository-toolbar">
      <SearchInput value={query} onChange={onQueryChange} label={t("repo.search")} placeholder={t("repo.searchPlaceholder")} />
      <div className="repository-toolbar-actions">
        <Tooltip content={t(opening ? "repo.choosingDescription" : "repo.open")} trigger={
          <button className="open-repository" type="button" onClick={onOpen} disabled={opening}>
            <PlusIcon aria-hidden="true" />{t(opening ? "repo.choosing" : "repo.open")}
          </button>
        } />
      </div>
    </div>
    <RepositoryList key={query.trim().toLocaleLowerCase()} {...list} />
  </>;
}
