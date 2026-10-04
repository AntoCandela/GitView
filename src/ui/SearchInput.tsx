/** Renders a controlled, accessibly named search field with an explicit clear action. */

import { CloseIcon, SearchIcon } from "./icons";
import { Tooltip } from "./Tooltip";
import { useTranslation } from "../i18n";

export function SearchInput({
  value,
  onChange,
  label,
  placeholder,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  placeholder: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="ui-search">
      <SearchIcon aria-hidden="true" />
      <input
        type="search"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-label={label}
        placeholder={placeholder}
        spellCheck={false}
      />
      {value ? (
        <Tooltip content={t("ui.clearSearch")} trigger={<button
          type="button"
          onClick={() => onChange("")}
          aria-label={t("ui.clearSearch")}
        >
          <CloseIcon aria-hidden="true" />
        </button>} />
      ) : null}
    </div>
  );
}
