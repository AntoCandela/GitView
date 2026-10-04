/** Presents locale selection without remounting the workspace or owning its review state. */
import { useRef } from "react";
import { Popover } from "@base-ui/react/popover";
import { setLocaleChoice, useLocale, useTranslation, type LocaleChoice } from "../i18n";
import { LanguageIcon } from "../ui/icons";
import { RadioGroup } from "../ui/RadioGroup";
import { Tooltip } from "../ui/Tooltip";

const languageOptions = [
  { value: "pt-BR", label: "Português (Brasil)" },
  { value: "pt-PT", label: "Português (Portugal)" },
  { value: "it", label: "Italiano" },
  { value: "es", label: "Español" },
  { value: "en-US", label: "English (United States)" },
  { value: "en-GB", label: "English (United Kingdom)" },
] as const;

export function LanguageMenu() {
  const { choice, pendingChoice, persistenceError } = useLocale();
  const { t } = useTranslation();
  const popup = useRef<HTMLDivElement>(null);
  return <Popover.Root>
    <Tooltip content={t("common.language")} trigger={
      <Popover.Trigger className="ui-kebab-trigger" aria-label={t("common.language")}>
        <LanguageIcon aria-hidden="true" />
      </Popover.Trigger>
    } />
    <Popover.Portal>
      <Popover.Positioner className="workbench-layout-positioner" side="bottom" align="end" sideOffset={4} collisionPadding={8}>
        <Popover.Popup ref={popup} className="workbench-layout-popup language-popup"
          initialFocus={() => popup.current?.querySelector<HTMLInputElement>("input:checked") ?? true}>
          <Popover.Title className="workbench-layout-title">{t("common.language")}</Popover.Title>
          <RadioGroup<LocaleChoice> label={t("common.language")} value={choice}
            options={[{ value: "system", label: t("common.system") }, ...languageOptions]}
            onChange={(next) => void setLocaleChoice(next)}
            onReselect={(next) => void setLocaleChoice(next)} />
          {pendingChoice ? <p role="status">{t("common.resolving")}</p> : null}
          {persistenceError ? <p role="alert">{t("common.sessionOnly")}</p> : null}
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  </Popover.Root>;
}
