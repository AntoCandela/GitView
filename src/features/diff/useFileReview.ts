/** Keeps comparison authority stable, polls live bytes without overlapping reads and rejects superseded choices. */

import { useEffect, useMemo, useRef, useState } from "react";
import type { ChangedPath } from "../../contracts/changes";
import type { ReviewCategory, ReviewIdentity, ReviewResult, ReviewSelection } from "../../contracts/diff";
import type { RepositoryClient } from "../../contracts/repositories";
import type { ObservationView } from "../changes";

export type FileReviewView =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "updating"; previous: Extract<ReviewResult, { kind: "text" }> }
  | { kind: "no_remaining" }
  | { kind: "transport_unavailable" }
  | { kind: "observation_unavailable" }
  | Extract<ReviewResult, { kind: "text" | "unsupported" | "unavailable" }>;

interface ReviewRequest {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  stablePathId: string;
  category: ReviewCategory;
  observationRevision: number;
  pathId: string;
}

function sameIdentity(first: ReviewIdentity, second: ReviewIdentity): boolean {
  return first.entryId === second.entryId && first.pathId === second.pathId && first.category === second.category
    && first.displayPath === second.displayPath && first.contextLabel === second.contextLabel
    && first.from === second.from && first.to === second.to
    && first.fromAbsent === second.fromAbsent && first.toAbsent === second.toAbsent;
}

function sameView(first: FileReviewView, second: FileReviewView): boolean {
  if (first.kind !== second.kind) return false;
  if (first.kind === "text" && second.kind === "text") {
    return sameIdentity(first, second) && first.fromContent === second.fromContent && first.toContent === second.toContent
      && first.hunks.length === second.hunks.length && first.hunks.every((hunk, index) => {
      const other = second.hunks[index];
      return hunk.oldStart === other.oldStart && hunk.oldCount === other.oldCount
        && hunk.newStart === other.newStart && hunk.newCount === other.newCount
        && hunk.lines.length === other.lines.length && hunk.lines.every((line, lineIndex) => {
          const otherLine = other.lines[lineIndex];
          return line.kind === otherLine.kind && line.text === otherLine.text && !!line.noFinalNewline === !!otherLine.noFinalNewline;
        });
    });
  }
  if (first.kind === "unsupported" && second.kind === "unsupported") return first.reason === second.reason && sameIdentity(first.identity, second.identity);
  if (first.kind === "unavailable" && second.kind === "unavailable") return first.code === second.code && sameIdentity(first.identity, second.identity);
  return first.kind !== "updating";
}

function sameScope(first: ReviewRequest, second: ReviewRequest): boolean {
  return first.client === second.client && first.entryId === second.entryId && first.selectionGeneration === second.selectionGeneration
    && first.stablePathId === second.stablePathId && first.category === second.category;
}

/** Presence is category-specific; unsupported files can still be reviewed for an explicit native explanation. */
export function hasReviewCategory(file: ChangedPath, selection: ReviewSelection): boolean {
  return file.conflict || file.unsupportedKind !== null || (selection.category === "untracked" ? file.untracked : file[selection.category] !== null);
}

/** Reads immediately on authority changes and checks live bytes again one second after each completion. */
export function useFileReview(
  client: RepositoryClient,
  entryId: string,
  selectionGeneration: number,
  observation: ObservationView,
  selection: ReviewSelection | null,
): FileReviewView {
  const file = observation.kind === "ready" && selection
    ? observation.files.find((candidate) => candidate.stablePathId === selection.stablePathId)
    : undefined;
  const stablePathId = selection?.stablePathId;
  const category = selection?.category;
  const observationRevision = observation.kind === "ready" ? observation.observationRevision : null;
  const pathId = observation.kind === "ready" && observation.entryId === entryId && selection && file && hasReviewCategory(file, selection)
    ? file.pathId : null;
  const request = useMemo<ReviewRequest | null>(() => {
    if (stablePathId === undefined || category === undefined || observationRevision === null || pathId === null) return null;
    return { client, entryId, selectionGeneration, stablePathId, category, observationRevision, pathId };
  }, [client, entryId, selectionGeneration, stablePathId, category, observationRevision, pathId]);
  const desired = useRef<ReviewRequest | null>(request);
  desired.current = request;
  const [running] = useState(() => new Map<RepositoryClient, ReviewRequest>());
  const [attempted] = useState(() => new WeakSet<ReviewRequest>());
  const mounted = useRef(false);
  const timer = useRef<number | undefined>(undefined);
  const [completed, setCompleted] = useState<{ request: ReviewRequest; view: FileReviewView } | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    function startLatest() {
      const latest = desired.current;
      if (!mounted.current || !latest || attempted.has(latest) || running.has(latest.client)) return;
      running.set(latest.client, latest);
      // An abandoned client's completion can wake the lane after the latest read already finished.
      attempted.add(latest);
      void (async () => {
        let view: FileReviewView;
        let awaitObservation = false;
        try {
          const result = await latest.client.reviewFile(latest.entryId, latest.observationRevision, latest.pathId, latest.category);
          if (result.kind === "stale_selection" || result.kind === "stale_observation") {
            view = { kind: "checking" };
            awaitObservation = true;
          } else {
            const identity = result.kind === "text" ? result : result.identity;
            const matches = identity.entryId === latest.entryId && identity.pathId === latest.pathId && identity.category === latest.category;
            view = matches ? result : { kind: "checking" };
            awaitObservation = !matches;
          }
        } catch {
          view = { kind: "transport_unavailable" };
        }
        // Render-time desired identity also invalidates completions before effect cleanup.
        if (mounted.current && desired.current === latest) {
          setCompleted((previous) => {
            // Stale authority cannot certify content, but need not erase this reading scope's last verified text.
            const retained = previous && sameScope(previous.request, latest)
              ? previous.view.kind === "text" ? previous.view : previous.view.kind === "updating" ? previous.view.previous : null : null;
            const nextView = awaitObservation && retained ? { kind: "updating" as const, previous: retained } : view;
            if (previous?.request === latest && sameView(previous.view, nextView)) return previous;
            return { request: latest, view: nextView };
          });
          if (!awaitObservation) {
            timer.current = window.setTimeout(() => {
              if (!mounted.current || desired.current !== latest) return;
              attempted.delete(latest);
              startLatest();
            }, 1000);
          }
        }
        if (running.get(latest.client) === latest) running.delete(latest.client);
        if (desired.current !== latest) startLatest();
      })();
    }
    startLatest();
    return () => {
      window.clearTimeout(timer.current);
      timer.current = undefined;
    };
  }, [request, running, attempted]);

  if (!selection) return { kind: "idle" };
  if (observation.kind === "transport_unavailable") return { kind: "transport_unavailable" };
  if (observation.entryId !== entryId) return { kind: "checking" };
  if (observation.kind === "unavailable" || observation.kind === "bare") return { kind: "observation_unavailable" };
  if (observation.kind === "checking") return { kind: "checking" };
  if (!request) return { kind: "no_remaining" };
  if (completed?.request === request) return completed.view;
  const retained = completed && sameScope(completed.request, request)
    ? completed.view.kind === "text" ? completed.view : completed.view.kind === "updating" ? completed.view.previous : null : null;
  if (retained) {
    return { kind: "updating", previous: retained };
  }
  return { kind: "checking" };
}
