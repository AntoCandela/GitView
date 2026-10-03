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

type SyntaxChoice = CodeTheme | "match";

export function AppearanceMenu() {
  const popup = useRef<HTMLDivElement>(null);
  const appearance = useAppearanceTheme();
  const icons = useIconTheme();
  const review = useReviewChoices();
  if (!icons) throw new Error("AppearanceMenu requires IconThemeProvider");
  const iconPreference = icons;
  const selected = appearanceTheme(appearance.theme);
  const syntax = review.theme === "match" ? selected.syntax : review.theme;
  const code = codeTheme(syntax);
  const palette = code.preview;
  const iconLabel = iconThemes.find(({ id }) => id === icons.theme)!.label;
  const preset = review.theme === "match" && icons.theme === selected.icons ? selected.id : null;

  function applyPreset(theme: AppearanceTheme) {
    const choice = appearanceTheme(theme);
    appearance.setTheme(theme);
    review.change({ theme: "match" });
    iconPreference.setTheme(choice.icons);
  }

  return <Popover.Root>
    <Tooltip content="Choose appearance" trigger={
      <Popover.Trigger className="ui-kebab-trigger" aria-label="Appearance"><AppearanceIcon aria-hidden="true" /></Popover.Trigger>
    } />
    <Popover.Portal>
      <Popover.Positioner className="ui-menu-positioner" side="bottom" align="end" sideOffset={4} collisionPadding={8}>
        <Popover.Popup ref={popup} className="ui-menu appearance-menu"
          initialFocus={() => popup.current?.querySelector<HTMLElement>('.appearance-presets button[aria-pressed="true"]')
            ?? popup.current?.querySelector<HTMLElement>(".appearance-section .ui-disclosure summary") ?? true}>
          <Popover.Title className="ui-choice-title">Appearance</Popover.Title>
          <div className="appearance-preview" role="img" aria-label={`Preview: ${selected.label} interface, ${code.label} syntax, ${iconLabel} icons`}>
            <div className="appearance-preview-heading"><TreeEntryIcon theme={icons.theme} kind="file" name="App.tsx" /> App.tsx <span>{selected.label}</span></div>
            <div className="appearance-preview-lines" style={{ "--code-bg": palette.background, "--code-ink": palette.foreground,
              "--code-add-bg": palette.addition, "--code-remove-bg": palette.removal } as CSSProperties}>
              <code><span>− </span><span style={{ color: palette.keyword }}>const</span> title = <span style={{ color: palette.string }}>"old"</span>;</code>
              <code><span>+ </span><span style={{ color: palette.keyword }}>const</span> title = <span style={{ color: palette.string }}>"new"</span>;</code>
            </div>
          </div>
          <section className="appearance-section" aria-label="Coordinated presets">
            <h3>Presets <small>{preset ? selected.label : "Custom"}</small></h3>
            <div className="appearance-presets">
              {appearanceThemes.map((theme) => <Tooltip key={theme.id} content={`Apply ${theme.label} appearance preset`} trigger={
                <button type="button" name="appearance-presets"
                aria-pressed={preset === theme.id} onClick={() => applyPreset(theme.id)}>
                <span className="appearance-swatch" style={{ background: theme.swatch, color: theme.ink }} aria-hidden="true">Aa</span>
                {theme.label}
              </button>} />)}
            </div>
          </section>
          <section className="appearance-section" aria-label="Independent choices">
            <h3>Customize <small>Choices stay independent</small></h3>
            <DisclosureSection title="Interface" detail={selected.label}>
            <RadioGroup<AppearanceTheme> label="Interface palette" value={appearance.theme}
              options={appearanceThemes.map(({ id, label, family, swatch, ink }) => ({ value: id, label,
                content: <span className="appearance-option"><span>{label} · {family}</span><span className="appearance-swatch" style={{ background: swatch, color: ink }} aria-hidden="true">Aa</span></span> }))}
              onChange={appearance.setTheme} />
            </DisclosureSection>
            <DisclosureSection title="Syntax" detail={review.theme === "match" ? `Match · ${code.label}` : code.label}>
            <RadioGroup<SyntaxChoice> label="Syntax palette" value={review.theme}
              options={[{ value: "match", label: "Match interface", content: <span>Match interface · {codeTheme(selected.syntax).label}</span> },
                ...codeThemes.map(({ id, label, preview }) => ({ value: id, label,
                  content: <span className="code-theme-choice"><span>{label}</span>
                    <code className="code-theme-preview" aria-hidden="true" style={{ background: preview.background, color: preview.foreground }}>
                      <span style={{ color: preview.keyword }}>const</span>{" = "}<span style={{ color: preview.string }}>{'"code"'}</span>
                    </code></span> }))]}
              onChange={(theme) => review.change({ theme })} />
            </DisclosureSection>
            <DisclosureSection title="File icons" detail={iconLabel}>
            <RadioGroup<IconTheme> label="File icons" value={icons.theme}
              options={iconThemes.map(({ id, label, preview }) => ({ value: id, label,
                content: <span className="icon-theme-choice"><span>{label}</span><span className="icon-theme-preview" aria-hidden="true">
                  <TreeEntryIcon theme={id} kind="folder" name={preview.folder} />
                  {preview.files.map((name) => <TreeEntryIcon key={name} theme={id} kind="file" name={name} />)}
                </span></span> }))}
              onChange={icons.setTheme} />
            </DisclosureSection>
          </section>
          <DisclosureSection title="Sources and licenses">
            {codeThemes.map(({ id, label, source }) => source && <a key={id} className="ui-choice-link" href={source} target="_blank" rel="noreferrer">{label} · upstream theme · MIT</a>)}
            <a className="ui-choice-link" href={themeNotices} target="_blank" rel="noreferrer">Theme licenses and original notices</a>
            {iconThemes.map(({ id, license }) => license && <a key={id} className="ui-choice-link" href={license.url} target="_blank" rel="noreferrer">{license.label}</a>)}
            <a className="ui-choice-link" href={grammarNotices} target="_blank" rel="noreferrer">TextMate grammar original notices</a>
            <a className="ui-choice-link" href={textmateLicense} target="_blank" rel="noreferrer">TextMate bundle license · YAML / TOML</a>
            <a className="ui-choice-link" href={shikiLicense} target="_blank" rel="noreferrer">Shiki 4.4.3 · MIT license</a>
            <a className="ui-choice-link" href={onigurumaLicense} target="_blank" rel="noreferrer">VS Code Oniguruma · Microsoft · MIT</a>
            <a className="ui-choice-link" href={onigurumaNotices} target="_blank" rel="noreferrer">Oniguruma · K. Kosako · BSD-2-Clause</a>
          </DisclosureSection>
          {appearance.persistenceError || review.persistenceError || icons.persistenceError
            ? <p className="ui-choice-warning" role="status">Choice applies to this session; storage is unavailable.</p> : null}
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  </Popover.Root>;
}
