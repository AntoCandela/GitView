/** Defines safe text-token reviews and the private worker protocol; source stays in memory only. */

import type { TextHunk } from "../../../contracts/diff";
import type { CodeTheme } from "../../appearance";
export type { CodeTheme } from "../../appearance";

/** Text is never HTML; fontStyle uses Shiki's italic/bold/underline/strikethrough bit flags. */
export interface Token {
  text: string;
  color?: string;
  fontStyle?: number;
}

/** Half-open UTF-16 offsets suitable for String.slice within one source line. */
export interface ChangedRange {
  start: number;
  end: number;
}

/** Tokens use zero-based source lines; ranges use native, one-based source line numbers. */
export interface HighlightedCode {
  old: Token[][];
  new: Token[][];
  oldChanges: Record<number, ChangedRange[]>;
  newChanges: Record<number, ChangedRange[]>;
}

export interface HighlightInput {
  fromContent: string;
  toContent: string;
  displayPath: string;
  hunks: TextHunk[];
}

export interface HighlightRequest {
  id: number;
  input: HighlightInput;
  theme: CodeTheme;
}

export type HighlightResponse =
  | { id: number; code: HighlightedCode; error: false }
  | { id: number; error: true };

/** Retains CR and Unicode; LF is a row delimiter, not an extra final source line. */
export function sourceLines(content: string): string[] {
  if (content === "") return [];
  const lines = content.split("\n");
  if (content.endsWith("\n")) lines.pop();
  return lines;
}

/** Poll snapshots may be new objects without changing the computation they authorize. */
export function sameHighlightInput(first: HighlightInput, second: HighlightInput): boolean {
  if (first.fromContent !== second.fromContent || first.toContent !== second.toContent
    || first.displayPath !== second.displayPath || first.hunks.length !== second.hunks.length) return false;
  return first.hunks === second.hunks || first.hunks.every((hunk, index) => {
    const other = second.hunks[index];
    return hunk.oldStart === other.oldStart && hunk.newStart === other.newStart
      && hunk.oldCount === other.oldCount && hunk.newCount === other.newCount
      && (hunk.lines === other.lines || hunk.lines.length === other.lines.length && hunk.lines.every((line, lineIndex) => {
        const otherLine = other.lines[lineIndex];
        return line.kind === otherLine.kind && line.text === otherLine.text
          && Boolean(line.noFinalNewline) === Boolean(otherLine.noFinalNewline);
      }));
  });
}
