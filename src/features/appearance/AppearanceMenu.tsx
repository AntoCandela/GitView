/** Offers coordinated presets and independent interface, syntax and icon choices with a live sample. */
import { useRef, type CSSProperties } from "react";
import { Popover } from "@base-ui/react/popover";
import grammarNotices from "../../../licenses/texts/assets/code-highlighting/grammars-NOTICE.txt?url&no-inline";
import themeNotices from "../../../licenses/texts/assets/code-highlighting/themes-NOTICE.txt?url&no-inline";
import shikiLicense from "../../../licenses/texts/assets/code-highlighting/shiki-LICENSE.txt?url&no-inline";
import onigurumaLicense from "../../../licenses/texts/assets/code-highlighting/oniguruma-LICENSE.txt?url&no-inline";
import onigurumaNotices from "../../../licenses/texts/assets/code-highlighting/oniguruma-NOTICES.txt?url&no-inline";
import textmateLicense from "../../../licenses/texts/assets/code-highlighting/yaml-LICENSE.txt?url&no-inline";
import { codeTheme, codeThemes, type CodeTheme } from "./codeThemes";
import { useReviewChoices } from "./reviewPreferences";
import { appearanceTheme, appearanceThemes, type AppearanceTheme } from "./appearanceThemes";
import { useAppearanceTheme } from "./appearancePreference";
import { useIconTheme } from "../../ui/file-icons/IconThemeProvider";
import { iconThemes, type IconTheme } from "../../ui/file-icons/iconThemes";
import { AppearanceIcon } from "../../ui/icons";
import { TreeEntryIcon } from "../../ui/file-icons/TreeEntryIcon";
import { DisclosureSection } from "../../ui/DisclosureSection";
import { RadioGroup } from "../../ui/RadioGroup";
import { Tooltip } from "../../ui/Tooltip";
import { useTranslation } from "../../i18n";

type SyntaxChoice = CodeTheme | "match";

export function AppearanceMenu() {
  const { t } = useTranslation();
  const popup = useRef<HTMLDivElement>(null);
  const appearance = useAppearanceTheme();
  const icons = useIconTheme();
  const review = useReviewChoices();
  if (!icons) throw new Error("AppearanceMenu requires IconThemeProvider");
  const iconPreference = icons;
  const selected = appearanceTheme(appearance.theme);
  const selectedLabel = t(selected.labelKey);
  const syntax = review.theme === "match" ? selected.syntax : review.theme;
  const code = codeTheme(syntax);
  const codeLabel = code.id === "plain" ? t("appearance.syntax.plain") : code.label;
  const palette = code.preview;
  const iconLabel = icons.theme === "classic" ? t("appearance.icons.classic") : iconThemes.find(({ id }) => id === icons.theme)!.label;
  const preset = review.theme === "match" && icons.theme === selected.icons ? selected.id : null;

  function applyPreset(theme: AppearanceTheme) {
    const choice = appearanceTheme(theme);
    appearance.setTheme(theme);
    review.change({ theme: "match" });
    iconPreference.setTheme(choice.icons);
  }

  return <Popover.Root>
    <Tooltip content={t("appearance.choose")} trigger={
      <Popover.Trigger className="ui-kebab-trigger" aria-label={t("appearance.title")}><AppearanceIcon aria-hidden="true" /></Popover.Trigger>
    } />
    <Popover.Portal>
      <Popover.Positioner className="ui-menu-positioner" side="bottom" align="end" sideOffset={4} collisionPadding={8}>
        <Popover.Popup ref={popup} className="ui-menu appearance-menu"
          initialFocus={() => popup.current?.querySelector<HTMLElement>('.appearance-presets button[aria-pressed="true"]')
            ?? popup.current?.querySelector<HTMLElement>(".appearance-section .ui-disclosure summary") ?? true}>
          <Popover.Title className="ui-choice-title">{t("appearance.title")}</Popover.Title>
          <div className="appearance-preview" role="img" aria-label={t("appearance.preview", { interface: selectedLabel, syntax: codeLabel, icons: iconLabel })}>
            <div className="appearance-preview-heading"><TreeEntryIcon theme={icons.theme} kind="file" name="App.tsx" /> App.tsx <span>{selectedLabel}</span></div>
            <div className="appearance-preview-lines" style={{ "--code-bg": palette.background, "--code-ink": palette.foreground,
              "--code-add-bg": palette.addition, "--code-remove-bg": palette.removal } as CSSProperties}>
              <code><span>− </span><span style={{ color: palette.keyword }}>const</span> title = <span style={{ color: palette.string }}>"old"</span>;</code>
              <code><span>+ </span><span style={{ color: palette.keyword }}>const</span> title = <span style={{ color: palette.string }}>"new"</span>;</code>
            </div>
          </div>
          <section className="appearance-section" aria-label={t("appearance.coordinatedPresets")}>
            <h3>{t("appearance.presets")} <small>{preset ? selectedLabel : t("appearance.custom")}</small></h3>
            <div className="appearance-presets">
              {appearanceThemes.map((theme) => <Tooltip key={theme.id} content={t("appearance.applyPreset", { theme: t(theme.labelKey) })} trigger={
                <button type="button" name="appearance-presets"
                aria-pressed={preset === theme.id} onClick={() => applyPreset(theme.id)}>
                <span className="appearance-swatch" style={{ background: theme.swatch, color: theme.ink }} aria-hidden="true">Aa</span>
                {t(theme.labelKey)}
              </button>} />)}
            </div>
          </section>
          <section className="appearance-section" aria-label={t("appearance.independentChoices")}>
            <h3>{t("appearance.customize")} <small>{t("appearance.choicesStayIndependent")}</small></h3>
            <DisclosureSection title={t("appearance.interface")} detail={selectedLabel}>
            <RadioGroup<AppearanceTheme> label={t("appearance.interfacePalette")} value={appearance.theme}
              options={appearanceThemes.map(({ id, labelKey, familyKey, swatch, ink }) => ({ value: id, label: t(labelKey),
                content: <span className="appearance-option"><span>{t("appearance.paletteFamily", { palette: t(labelKey), family: t(familyKey) })}</span><span className="appearance-swatch" style={{ background: swatch, color: ink }} aria-hidden="true">Aa</span></span> }))}
              onChange={appearance.setTheme} />
            </DisclosureSection>
            <DisclosureSection title={t("appearance.syntax")} detail={review.theme === "match" ? t("appearance.matchSyntax", { syntax: codeLabel }) : codeLabel}>
            <RadioGroup<SyntaxChoice> label={t("appearance.syntaxPalette")} value={review.theme}
              options={[{ value: "match", label: t("appearance.matchInterface"), content: <span>{t("appearance.matchInterfaceSyntax", { syntax: codeTheme(selected.syntax).label })}</span> },
                ...codeThemes.map(({ id, label, preview }) => ({ value: id, label: id === "plain" ? t("appearance.syntax.plain") : label,
                  content: <span className="code-theme-choice"><span>{id === "plain" ? t("appearance.syntax.plain") : label}</span>
                    <code className="code-theme-preview" aria-hidden="true" style={{ background: preview.background, color: preview.foreground }}>
                      <span style={{ color: preview.keyword }}>const</span>{" = "}<span style={{ color: preview.string }}>{'"code"'}</span>
                    </code></span> }))]}
              onChange={(theme) => review.change({ theme })} />
            </DisclosureSection>
            <DisclosureSection title={t("appearance.fileIcons")} detail={iconLabel}>
            <RadioGroup<IconTheme> label={t("appearance.fileIcons")} value={icons.theme}
              options={iconThemes.map(({ id, label, preview }) => ({ value: id, label: id === "classic" ? t("appearance.icons.classic") : label,
                content: <span className="icon-theme-choice"><span>{id === "classic" ? t("appearance.icons.classic") : label}</span><span className="icon-theme-preview" aria-hidden="true">
                  <TreeEntryIcon theme={id} kind="folder" name={preview.folder} />
                  {preview.files.map((name) => <TreeEntryIcon key={name} theme={id} kind="file" name={name} />)}
                </span></span> }))}
              onChange={icons.setTheme} />
            </DisclosureSection>
          </section>
          <DisclosureSection title={t("appearance.sourcesLicenses")}>
            {codeThemes.map(({ id, label, source }) => source && <a key={id} className="ui-choice-link" href={source} target="_blank" rel="noreferrer">{t("appearance.upstreamTheme", { theme: label })}</a>)}
            <a className="ui-choice-link" href={themeNotices} target="_blank" rel="noreferrer">{t("appearance.themeNotices")}</a>
            {iconThemes.map(({ id, license }) => license && <a key={id} className="ui-choice-link" href={license.url} target="_blank" rel="noreferrer">{t("appearance.mitLicense", { name: id === "material" ? "Material Icon Theme" : "Catppuccin Icons" })}</a>)}
            <a className="ui-choice-link" href={grammarNotices} target="_blank" rel="noreferrer">{t("appearance.grammarNotices")}</a>
            <a className="ui-choice-link" href={textmateLicense} target="_blank" rel="noreferrer">{t("appearance.textmateLicense")}</a>
            <a className="ui-choice-link" href={shikiLicense} target="_blank" rel="noreferrer">{t("appearance.mitLicense", { name: "Shiki 4.4.3" })}</a>
            <a className="ui-choice-link" href={onigurumaLicense} target="_blank" rel="noreferrer">VS Code Oniguruma · Microsoft · MIT</a>
            <a className="ui-choice-link" href={onigurumaNotices} target="_blank" rel="noreferrer">Oniguruma · K. Kosako · BSD-2-Clause</a>
          </DisclosureSection>
          {appearance.persistenceError || review.persistenceError || icons.persistenceError
            ? <p className="ui-choice-warning" role="status">{t("appearance.storageUnavailable")}</p> : null}
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  </Popover.Root>;
}
