/** Installs only the test transport before bootstrap; the production adapter and native service stay unchanged. */

import type { NativeJourney } from "./nativeJourney.ts";
import type { SurfaceNotice } from "../../src/contracts/companion";

/** Deliberately limited to APIs shared by Playwright and a Puppeteer adapter. */
export interface BrowserJourneyPage {
  exposeFunction(name: string, callback: (command: unknown, args?: unknown) => Promise<unknown>): Promise<unknown>;
  addInitScript(script: () => void): Promise<unknown>;
  evaluate(script: (notice: SurfaceNotice) => void, notice: SurfaceNotice): Promise<unknown>;
}

const commands: Record<string, readonly string[]> = {
  workspace_snapshot: [],
  preferred_languages: [],
  review_surface_bootstrap: [],
  review_surface_snapshot: [],
  subscribe_review_surface: [],
  companion_state: [],
  publish_companion_presentation: ["presentation"],
  pending_review_handoff: [],
  claim_review_handoff: ["requestId", "contextEpoch"],
  ack_review_handoff: ["requestId", "contextEpoch", "outcome"],
  open_chosen_repository: ["locale"],
  select_context: ["entryId"],
  refresh_entry_availability: ["entryId"],
  observe_selected_context: ["entryId"],
  review_file: ["entryId", "observationRevision", "pathId", "category"],
  history_page: ["entryId", "cursor", "branch"],
  list_contexts: ["entryId"],
  select_worktree: ["entryId", "worktreeId"],
  commit_files: ["entryId", "commitOid", "parentOid"],
  review_commit_file: ["entryId", "commitOid", "parentOid", "fileId"],
  list_repository_files: ["entryId", "request"],
  review_repository_file: ["entryId", "listingId", "fileId"],
  rename_repository: ["entryId", "displayName"],
  remove_repository: ["entryId"],
  record_renderer_diagnostic: ["diagnostic"],
  diagnostic_health: [],
};
const installedPages = new WeakSet<BrowserJourneyPage>();
const installedJourneys = new WeakSet<NativeJourney>();

/**
 * Call on a fresh page before navigation. Each page owns a distinct journey and one fixed main-picker choice.
 * Fixture control commands remain Node-only. The caller owns fixture_verify and close, including on failure.
 * Native diagnostic calls reject truthfully; RepositoryClient retains its best-effort diagnostic isolation.
 */
export async function installBrowserJourney(page: BrowserJourneyPage, journey: NativeJourney): Promise<void> {
  if (installedPages.has(page) || installedJourneys.has(journey)) throw new Error("Browser journey already installed");
  installedPages.add(page);
  installedJourneys.add(journey);
  await journey.request("fixture_choose", { repository: "main" });
  journey.onSurfaceNotice(async (notice) => {
    await page.evaluate((incoming) => {
      if (!("__gitviewJourneyNotice" in window) || typeof window.__gitviewJourneyNotice !== "function")
        throw new Error("Browser journey notice receiver unavailable");
      window.__gitviewJourneyNotice(incoming);
    }, notice);
  });
  await page.exposeFunction("__gitviewJourneyInvoke", async (command, args = {}) => {
    if (typeof command !== "string" || !Object.hasOwn(commands, command)) throw new Error("Unsupported browser journey command");
    if (args === null || typeof args !== "object" || Array.isArray(args)) throw new Error("Invalid browser journey arguments");
    const keys = commands[command];
    if (Object.keys(args).some((key) => key !== "operationId" && !keys.includes(key))) throw new Error("Invalid browser journey arguments");
    return journey.request(command, args as Record<string, unknown>);
  });
  await page.addInitScript(() => {
    const scope = window as unknown as {
      __gitviewJourneyInvoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
      __gitviewJourneyNotice: (notice: SurfaceNotice) => void;
    };
    const callbacks = new Map<number, (message: { index: number; message: SurfaceNotice }) => void>();
    let nextCallbackId = 1;
    let channelId: number | null = null;
    let messageIndex = 0;
    scope.__gitviewJourneyNotice = (notice) => {
      if (channelId !== null) callbacks.get(channelId)?.({ index: messageIndex++, message: notice });
    };
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: Object.freeze({
        invoke: (command: string, args?: Record<string, unknown>) => {
          if (command === "subscribe_review_surface") {
            const channel = args?.channel;
            if (!channel || typeof channel !== "object" || !("id" in channel) || typeof channel.id !== "number")
              return Promise.reject(new Error("Invalid browser journey channel"));
            if (channelId !== null) callbacks.delete(channelId);
            channelId = channel.id;
            messageIndex = 0;
            return scope.__gitviewJourneyInvoke(command);
          }
          return scope.__gitviewJourneyInvoke(command, args);
        },
        transformCallback: (callback: (message: { index: number; message: SurfaceNotice }) => void) => {
          const id = nextCallbackId++;
          callbacks.set(id, callback);
          return id;
        },
        unregisterCallback: (id: number) => { callbacks.delete(id); },
      }),
      configurable: false,
      writable: false,
    });
  });
}
