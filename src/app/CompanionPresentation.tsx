/** Bridges main-owned preferences to native presentation and renders compact consumers without storage authority. */
import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import type { CompanionSettingsClient, PresentationInput, PresentationSnapshot } from "../contracts/companion";
import { ReadOnlyAppearanceProvider, ReadOnlyReviewProvider, useAppearanceTheme, useReviewChoices } from "../features/appearance";
import { ReadOnlyLocaleProvider, useLocale, useLocaleReady, useTranslation } from "../i18n";
import { companionSettingsClient } from "../platform/RepositoryClient";
import { ReadOnlyIconThemeProvider, useIconTheme } from "../ui/file-icons/IconThemeProvider";
import { Tooltip } from "../ui/Tooltip";

let publicationRevision = 0;
const publicationListeners = new Set<() => void>();
const getPublicationRevision = () => publicationRevision;
function subscribePublication(listener: () => void) {
  publicationListeners.add(listener);
  return () => { publicationListeners.delete(listener); };
}

/** Signals completed native activation; it carries no preferences or surface authority. */
export function useCompanionPublicationRevision() {
  return useSyncExternalStore(subscribePublication, getPublicationRevision, getPublicationRevision);
}

/** Mount inside the main IconThemeProvider and retain it while main is hidden. */
export function CompanionPresentationPublisher({ client = companionSettingsClient }: {
  client?: Pick<CompanionSettingsClient, "publishPresentation">;
}) {
  const locale = useLocale();
  const ready = useLocaleReady();
  const appearance = useAppearanceTheme();
  const review = useReviewChoices();
  const icons = useIconTheme();
  const { t } = useTranslation();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const queue = useRef(Promise.resolve());
  const generation = useRef(0);
  const iconTheme = icons?.theme;
  const persistenceError = locale.persistenceError || appearance.persistenceError || review.persistenceError || Boolean(icons?.persistenceError);
  const presentation = useMemo<PresentationInput | null>(() => ready && iconTheme ? {
    locale: locale.locale, appearanceTheme: appearance.theme, iconTheme,
    review: { mode: review.mode, theme: review.theme, lineMode: review.lineMode }, persistenceError,
    menuLabels: { openGitView: t("companion.openGitView"), quit: t("companion.quit") },
  } : null, [ready, locale.locale, appearance.theme, iconTheme, review.mode, review.theme, review.lineMode, persistenceError, t]);

  useEffect(() => {
    if (!presentation) return;
    const publication = ++generation.current;
    let active = true;
    // Serialize IPC so an older session choice cannot arrive after its replacement.
    queue.current = queue.current.then(async () => {
      if (!active) return;
      try {
        await client.publishPresentation(presentation);
        ++publicationRevision;
        for (const listener of publicationListeners) listener();
        if (active && publication === generation.current) setFailed(false);
      } catch {
        if (active && publication === generation.current) setFailed(true);
      }
    });
    return () => { active = false; };
  }, [client, presentation, attempt]);

  if (!icons) throw new Error("Companion publisher requires the main icon preference owner");
  return failed ? <div role="alert"><p>{t("companion.presentationUnavailable")}</p>
    <Tooltip content={t("companion.retry")} trigger={<button type="button" className="companion-action"
      onClick={() => setAttempt((value) => value + 1)}>{t("companion.retry")}</button>} /></div> : null;
}

/** A missing first snapshot must not expose untranslated content or Classic/default presentation. */
export function CompanionPresentationProvider({ presentation, children }: {
  presentation: PresentationSnapshot | null; children: ReactNode;
}) {
  useLayoutEffect(() => {
    if (presentation) document.documentElement.dataset.appearance = presentation.appearanceTheme;
  }, [presentation?.appearanceTheme]);
  if (!presentation) return <div role="status" aria-busy="true" />;
  return <ReadOnlyLocaleProvider locale={presentation.locale} persistenceError={presentation.persistenceError}>
    <ReadOnlyAppearanceProvider theme={presentation.appearanceTheme} persistenceError={presentation.persistenceError}>
      <ReadOnlyReviewProvider review={presentation.review} persistenceError={presentation.persistenceError}>
        <ReadOnlyIconThemeProvider theme={presentation.iconTheme} persistenceError={presentation.persistenceError}>
          {children}
        </ReadOnlyIconThemeProvider>
      </ReadOnlyReviewProvider>
    </ReadOnlyAppearanceProvider>
  </ReadOnlyLocaleProvider>;
}
