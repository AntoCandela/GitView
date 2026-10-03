/** Tokenizes complete endpoint grammars off the UI thread and bounds per-line inline comparison work. */

import { diffWordsWithSpace } from "diff";
import { createHighlighterCore } from "shiki/core";
import type { HighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import { sourceLines } from "./highlighting";
import type { ChangedRange, CodeTheme, HighlightedCode, HighlightRequest, HighlightResponse, Token } from "./highlighting";
import { languageForPath, languages } from "./languages";
import type { Language } from "./languages";
import { codeThemeLoaders } from "./codeThemeLoaders";


const MAX_INLINE_LINE_LENGTH = 4096;
const MAX_INLINE_PAIRS = 2000;
const INLINE_WORK_MS = 100;
const MAX_TOKENIZED_CHARACTERS = 2 * 1024 * 1024;
const MAX_STYLED_TOKENS_PER_LINE = 512;
let highlighter: Promise<HighlighterCore> | null = null;
let cachedEndpoints: { from: string; to: string; lang: Language | null; theme: CodeTheme;
  old: Token[][]; new: Token[][] } | null = null;

function plainTokens(content: string): Token[][] {
  return sourceLines(content).map((text) => [{ text }]);
}

async function endpointTokens(request: HighlightRequest): Promise<Pick<HighlightedCode, "old" | "new">> {
  const { fromContent, toContent } = request.input;
  const lang = languageForPath(request.input.displayPath);
  const theme = request.theme;
  if (cachedEndpoints?.from === fromContent && cachedEndpoints.to === toContent
    && cachedEndpoints.lang === lang && cachedEndpoints.theme === theme) return cachedEndpoints;
  let old: Token[][];
  let next: Token[][];
  if (theme === "plain" || lang === null || fromContent.length + toContent.length > MAX_TOKENIZED_CHARACTERS) {
    old = plainTokens(fromContent);
    next = fromContent === toContent ? old : plainTokens(toContent);
  } else {
    highlighter ??= createHighlighterCore({
      langs: [], themes: [], engine: createOnigurumaEngine(import("shiki/wasm")),
    });
    const instance = await highlighter;
    await Promise.all([
      instance.getLoadedLanguages().includes(lang) ? undefined : instance.loadLanguage(languages[lang]),
      instance.getLoadedThemes().includes(theme) ? undefined : instance.loadTheme(codeThemeLoaders[theme]),
    ]);
    function tokenize(content: string): Token[][] {
      if (!content) return [];
      const lines = sourceLines(content);
      // Per-line truncation can retain partial comment/string state and falsely dim later source.
      // The transport's hard worker deadline bounds complete parsing instead.
      const tokens = instance.codeToTokens(content, { lang: lang!, theme, tokenizeTimeLimit: 0 }).tokens;
      return lines.map((line, index) => {
        // A dense/minified line remains exact plain text instead of flooding the UI with spans.
        // Parsing still covers that entire line so subsequent grammar state stays truthful.
        if ((tokens[index]?.length ?? 0) > MAX_STYLED_TOKENS_PER_LINE) return [{ text: line }];
        const row: Token[] = (tokens[index] ?? []).map((token) => ({
          text: token.content, color: token.color, fontStyle: token.fontStyle,
        }));
        // Shiki removes CR from CRLF delimiters; native source rows retain that byte.
        if (line.endsWith("\r") && (index < lines.length - 1 || content.endsWith("\n"))) {
          row.push({ text: "\r" });
        }
        return row;
      });
    }
    old = tokenize(fromContent);
    next = fromContent === toContent ? old : tokenize(toContent);
  }
  // A single pair bounds retained repository source, even across many themes and languages.
  cachedEndpoints = { from: fromContent, to: toContent, lang, theme, old, new: next };
  return cachedEndpoints;
}

function inlineChanges(request: HighlightRequest): Pick<HighlightedCode, "oldChanges" | "newChanges"> {
  const oldChanges: Record<number, ChangedRange[]> = {};
  const newChanges: Record<number, ChangedRange[]> = {};
  const oldLines = sourceLines(request.input.fromContent);
  const newLines = sourceLines(request.input.toContent);
  const deadline = performance.now() + INLINE_WORK_MS;
  let pairs = 0;
  for (const hunk of request.input.hunks) {
    let oldNumber = hunk.oldStart;
    let newNumber = hunk.newStart;
    let removals: number[] = [];
    let additions: number[] = [];
    function flushChanges() {
      for (let index = 0; index < Math.min(removals.length, additions.length); index++) {
        if (++pairs > MAX_INLINE_PAIRS || performance.now() >= deadline) break;
        const oldLine = oldLines[removals[index] - 1];
        const newLine = newLines[additions[index] - 1];
        if (oldLine === undefined || newLine === undefined || oldLine === newLine
          || oldLine.length > MAX_INLINE_LINE_LENGTH || newLine.length > MAX_INLINE_LINE_LENGTH) continue;
        const changes = diffWordsWithSpace(oldLine, newLine, {
          maxEditLength: 256, timeout: Math.min(5, Math.max(1, deadline - performance.now())),
        });
        if (!changes) continue;
        const removed: ChangedRange[] = [];
        const added: ChangedRange[] = [];
        let oldOffset = 0;
        let newOffset = 0;
        for (const change of changes) {
          if (change.removed) removed.push({ start: oldOffset, end: oldOffset + change.value.length });
          else if (change.added) added.push({ start: newOffset, end: newOffset + change.value.length });
          if (!change.added) oldOffset += change.value.length;
          if (!change.removed) newOffset += change.value.length;
        }
        if (removed.length) oldChanges[removals[index]] = removed;
        if (added.length) newChanges[additions[index]] = added;
      }
      removals = [];
      additions = [];
    }
    for (const line of hunk.lines) {
      if (line.kind === "context") {
        flushChanges();
        oldNumber++;
        newNumber++;
      } else if (line.kind === "removal") removals.push(oldNumber++);
      else additions.push(newNumber++);
    }
    flushChanges();
    if (pairs >= MAX_INLINE_PAIRS || performance.now() >= deadline) break;
  }
  return { oldChanges, newChanges };
}

const workerScope = self as unknown as {
  onmessage: ((event: MessageEvent<HighlightRequest>) => void) | null;
  postMessage(response: HighlightResponse): void;
};

// The UI transport admits one running job, retaining latest-only work per mounted consumer.
workerScope.onmessage = async ({ data: request }) => {
  try {
    const endpoints = await endpointTokens(request);
    workerScope.postMessage({ id: request.id, code: {
      old: endpoints.old, new: endpoints.new, ...inlineChanges(request),
    }, error: false });
  } catch {
    // Never send exception text: grammar errors may contain repository source.
    workerScope.postMessage({ id: request.id, error: true });
  }
};
