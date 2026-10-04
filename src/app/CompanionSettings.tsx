/** Presents native opt-in intent and reachability; it never owns workspace or persisted preference state. */
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Popover } from "@base-ui/react/popover";
import type { CompanionSettingsClient, CompanionState } from "../contracts/companion";
import { useTranslation } from "../i18n";
import { companionSettingsClient } from "../platform/RepositoryClient";
import { AlertIcon } from "../ui/icons";
import { Tooltip } from "../ui/Tooltip";
import { useCompanionPublicationRevision } from "./CompanionPresentation";

type SettingsClient = Pick<CompanionSettingsClient, "state" | "setEnabled">;

export function CompanionSettings({ client = companionSettingsClient, refreshRevision = 0 }: {
  client?: SettingsClient;
  /** The existing main surface subscription supplies its monotonic invalidation counter. */
  refreshRevision?: number;
}) {
  const { t } = useTranslation();
  const publicationRevision = useCompanionPublicationRevision();
  const [state, setState] = useState<CompanionState | null>(null);
  const [busy, setBusy] = useState(false);
  const [transportError, setTransportError] = useState(false);
  const generation = useRef(0);
  const changing = useRef(false);
  const requestedEnabled = useRef<boolean | null>(null);
  const descriptionId = useId();
  const checkbox = useRef<HTMLInputElement>(null);

  const refresh = useCallback(async () => {
    if (changing.current) return;
    const request = ++generation.current;
    try {
      const next = await client.state();
      if (request === generation.current) {
        setState((current) => current && current.revision > next.revision ? current : next);
        setTransportError(false);
      }
    } catch {
      if (request === generation.current) setTransportError(true);
    }
  }, [client]);

  useEffect(() => {
    const onFocus = () => { void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => { ++generation.current; window.removeEventListener("focus", onFocus); };
  }, [refresh]);

  useEffect(() => { void refresh(); }, [refresh, refreshRevision, publicationRevision]);

  async function change(enabled: boolean) {
    if (changing.current) return;
    requestedEnabled.current = enabled;
    changing.current = true;
    setBusy(true);
    const request = ++generation.current;
    try {
      const outcome = await client.setEnabled(enabled);
      if (request === generation.current) {
        setState(outcome.state);
        setTransportError(false);
      }
    } catch {
      if (request === generation.current) setTransportError(true);
    } finally {
      changing.current = false;
      if (request === generation.current) setBusy(false);
    }
  }

  if (state?.supported === false || (!state && !transportError)) return null;
  const protectedPreference = state?.persistenceError === "load_failed" || state?.persistenceError === "unsupported_version";
  const nativeUnavailable = Boolean(state?.nativeError) || Boolean(state?.enabled && !state.available);
  const hasError = transportError || nativeUnavailable || Boolean(state?.persistenceError);
  return <Popover.Root onOpenChange={(open) => { if (open) void refresh(); }}>
    <Tooltip content={t("companion.settings")} trigger={
      <Popover.Trigger className="companion-settings-trigger" aria-label={t("companion.settings")}>
        {hasError ? <AlertIcon aria-hidden="true" /> : null}{t("companion.settings")}
      </Popover.Trigger>
    } />
    <Popover.Portal>
      <Popover.Positioner className="workbench-layout-positioner" side="bottom" align="end" sideOffset={4} collisionPadding={8}>
        <Popover.Popup className="workbench-layout-popup companion-settings-popup" initialFocus={() => checkbox.current ?? true}>
          <Popover.Title className="workbench-layout-title">{t("companion.settings")}</Popover.Title>
          <p id={descriptionId}>{t("companion.description")}</p>
          <label><input ref={checkbox} type="checkbox" checked={state?.enabled ?? false} disabled={busy || !state}
            aria-describedby={descriptionId} onChange={(event) => void change(event.currentTarget.checked)} />
            {t("companion.enable")}</label>
          {busy ? <p role="status">{t("companion.updating")}</p> : null}
          {state?.available && !nativeUnavailable ? <p role="status">{t("companion.available")}</p> : null}
          {state?.persistenceError ? <p role="alert">{t(protectedPreference ? "companion.preferenceProtected" : "companion.sessionOnly")}</p> : null}
          {nativeUnavailable ? <p role="alert">{t(state?.nativeError === "main_unavailable" ? "companion.mainUnavailable" : "companion.nativeUnavailable")}</p> : null}
          {transportError ? <p role="alert">{t("companion.settingsUnavailable")}</p> : null}
          {hasError ? <Tooltip content={t("companion.retry")} trigger={<button type="button" className="companion-action" disabled={busy}
            onClick={() => {
              if (state && (!protectedPreference || nativeUnavailable || transportError)) void change(requestedEnabled.current ?? state.enabled);
              else void refresh();
            }}>{t("companion.retry")}</button>} /> : null}
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  </Popover.Root>;
}
