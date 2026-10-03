/** Owns one shared, disposable highlighting worker; render-time scopes reject obsolete completions. */

import { useEffect, useRef, useState } from "react";
import { sameHighlightInput } from "./highlighting";
import type { CodeTheme, HighlightedCode, HighlightInput, HighlightRequest, HighlightResponse } from "./highlighting";

interface HighlightScope {
  input: HighlightInput;
  theme: CodeTheme;
}
interface HighlightJob extends HighlightRequest {
  consumer: symbol;
  complete: (response: HighlightResponse) => void;
}
interface CompletedHighlight {
  scope: HighlightScope;
  code: HighlightedCode | null;
  error: boolean;
}

const consumers = new Set<symbol>();
const pending = new Map<symbol, HighlightJob>();
const WORKER_DEADLINE_MS = 15_000;
let worker: Worker | null = null;
let running: HighlightJob | null = null;
let deadline: number | undefined;
let requestId = 0;

function stopWorker() {
  clearTimeout(deadline);
  deadline = undefined;
  worker?.terminate();
  worker = null;
  running = null;
}

function failWorker() {
  const failed = running;
  stopWorker();
  const queued = [...pending.values()];
  pending.clear();
  if (failed && consumers.has(failed.consumer)) failed.complete({ id: failed.id, error: true });
  for (const job of queued) {
    if (consumers.has(job.consumer)) job.complete({ id: job.id, error: true });
  }
}

function startLatest() {
  if (running || pending.size === 0 || consumers.size === 0) return;
  const job = pending.values().next().value;
  if (!job) return;
  pending.delete(job.consumer);
  running = job;
  try {
    if (!worker) {
      const instance = new Worker(new URL("./highlighting.worker.ts", import.meta.url), { type: "module" });
      worker = instance;
      instance.onmessage = ({ data }: MessageEvent<HighlightResponse>) => {
        if (worker !== instance) return;
        if (!running || data.id !== running.id) {
          failWorker();
          return;
        }
        const completed = running;
        running = null;
        clearTimeout(deadline);
        deadline = undefined;
        if (consumers.has(completed.consumer)) completed.complete(data);
        startLatest();
      };
      instance.onerror = (event) => {
        // Do not let a browser console expose worker errors containing private source.
        event.preventDefault();
        if (worker === instance) failWorker();
      };
      instance.onmessageerror = () => {
        if (worker === instance) failWorker();
      };
    }
    // At most one payload enters the worker's event queue; changes replace pending UI-side work.
    worker.postMessage({ id: job.id, input: job.input, theme: job.theme } satisfies HighlightRequest);
    deadline = window.setTimeout(failWorker, WORKER_DEADLINE_MS);
  } catch {
    failWorker();
  }
}

function releaseConsumer(consumer: symbol) {
  consumers.delete(consumer);
  pending.delete(consumer);
  if (consumers.size === 0) {
    pending.clear();
    stopWorker();
  }
}

/**
 * Returns null tokens immediately on review/theme changes so callers can render native plain text.
 * Equivalent poll snapshots reuse their result; the last unmount terminates the worker and its source cache.
 */
export function useCodeHighlight(input: HighlightInput | null, theme: CodeTheme): {
  code: HighlightedCode | null;
  error: boolean;
} {
  const [consumer] = useState(() => Symbol("code-highlight"));
  const desired = useRef<HighlightScope | null>(null);
  const [completed, setCompleted] = useState<CompletedHighlight | null>(null);
  if (!input) desired.current = null;
  else if (!desired.current || desired.current.theme !== theme || !sameHighlightInput(desired.current.input, input)) {
    desired.current = { input, theme };
  }
  const scope = desired.current;

  useEffect(() => {
    consumers.add(consumer);
    return () => releaseConsumer(consumer);
  }, [consumer]);

  useEffect(() => {
    if (!scope) return;
    const job: HighlightJob = {
      id: ++requestId,
      consumer,
      input: scope.input,
      theme: scope.theme,
      complete: (response) => {
        // A new render invalidates old results before passive-effect cleanup has run.
        if (desired.current !== scope || !consumers.has(consumer)) return;
        setCompleted({ scope, code: response.error ? null : response.code, error: response.error });
      },
    };
    pending.set(consumer, job);
    startLatest();
    return () => {
      if (pending.get(consumer) === job) pending.delete(consumer);
    };
  }, [consumer, scope]);

  return completed?.scope === scope && scope !== null
    ? { code: completed.code, error: completed.error }
    : { code: null, error: false };
}
