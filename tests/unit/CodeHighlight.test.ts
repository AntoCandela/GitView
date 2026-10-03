/** Exercises genuine worker grammars, endpoint fidelity and bounded paired inline changes. */

import { afterAll, beforeAll, expect, test, vi } from "vitest";
import type { TextHunk } from "../../src/contracts/diff";
import type { CodeTheme, HighlightedCode, HighlightInput, HighlightRequest, HighlightResponse } from "../../src/features/diff/highlighting/highlighting";
import { languageForPath } from "../../src/features/diff/highlighting/languages";

let receive: ((event: MessageEvent<HighlightRequest>) => void) | null = null;
let complete: ((response: HighlightResponse) => void) | null = null;
let requestId = 0;

beforeAll(async () => {
  vi.stubGlobal("self", {
    get onmessage() { return receive; },
    set onmessage(callback: ((event: MessageEvent<HighlightRequest>) => void) | null) { receive = callback; },
    postMessage(response: HighlightResponse) { complete?.(response); },
  });
  // Exercise worker module initialization after installing its transport-only test boundary.
  await import("../../src/features/diff/highlighting/highlighting.worker");
});
afterAll(() => vi.unstubAllGlobals());

async function highlight(input: HighlightInput, theme: CodeTheme = "github-light"): Promise<HighlightedCode> {
  const response = await new Promise<HighlightResponse>((resolve) => {
    complete = resolve;
    receive!({ data: { id: ++requestId, input, theme } } as MessageEvent<HighlightRequest>);
  });
  if (response.error) throw new Error("Highlighting failed");
  return response.code;
}

function replacement(oldStart: number, newStart: number, removed: string[], added: string[]): TextHunk {
  return { oldStart, oldCount: removed.length, newStart, newCount: added.length, lines: [
    ...removed.map((text) => ({ kind: "removal" as const, text })),
    ...added.map((text) => ({ kind: "addition" as const, text })),
  ] };
}

test("both TSX endpoints keep syntax contrast after a closed documentation block", async () => {
  const prefix = '/** Component owner.\n * Complete header before source.\n */\nimport { useState } from "react";\n';
  const code = await highlight({ displayPath: "Component.tsx", fromContent: `${prefix}export const value = <div>Old</div>;\n`,
    toContent: `${prefix}export const value = <div>New</div>;\n`, hunks: [] }, "solarized-light");
  const oldImport = code.old[3].find((token) => token.text === "import")?.color;
  const newImport = code.new[3].find((token) => token.text === "import")?.color;
  const comment = code.old[1][0].color;
  expect(oldImport).toBeDefined();
  expect(oldImport).toBe(newImport);
  expect(oldImport).not.toBe(comment);
  expect(code.old[4].find((token) => token.text === "export")?.color)
    .toBe(code.new[4].find((token) => token.text === "export")?.color);
});

test("multiline context outside separated hunks colors each full endpoint independently", async () => {
  const code = await highlight({
    displayPath: "example.ts",
    fromContent: "/* opening\nconst hidden = 1;\n*/\nconst visible = 2;\n",
    toContent: "// opening\nconst hidden = 3;\n// ending\nconst visible = 4;\n",
    hunks: [replacement(2, 2, ["const hidden = 1;"], ["const hidden = 3;"]),
      replacement(4, 4, ["const visible = 2;"], ["const visible = 4;"])],
  });
  const commentColor = code.old[0][0].color;
  expect(commentColor).toBeDefined();
  expect(code.old[1].every((token) => token.color === commentColor)).toBe(true);
  const keywordColor = code.new[1].find((token) => token.text === "const")?.color;
  expect(keywordColor).toBeDefined();
  expect(keywordColor).not.toBe(commentColor);
  expect(code.old[3].find((token) => token.text === "const")?.color).toBe(keywordColor);
  expect(code.new[3].find((token) => token.text === "const")?.color).toBe(keywordColor);
});

test("tokens retain CRLF content, empty lines, Unicode and an unterminated final line", async () => {
  const content = 'const label = "界😀";\r\n\r\nconst tail = "é";';
  const code = await highlight({ displayPath: "declarations.d.mts", fromContent: "", toContent: content, hunks: [] });
  expect(code.old).toEqual([]);
  expect(code.new.map((line) => line.map((token) => token.text).join("")))
    .toEqual(['const label = "界😀";\r', "\r", 'const tail = "é";']);
  expect(code.new[0].find((token) => token.text === "const")?.color).toBeDefined();
});

test("plain style still marks side-specific word and whitespace changes with native line numbers", async () => {
  const code = await highlight({
    displayPath: "example.ts", fromContent: "heading\nlet  a = 1;\nold unmatched\n",
    toContent: "heading\nlet a = 2;\n",
    hunks: [replacement(2, 2, ["let  a = 1;", "old unmatched"], ["let a = 2;"])],
  }, "plain");
  expect(code.oldChanges).toEqual({ 2: [{ start: 3, end: 5 }, { start: 9, end: 10 }] });
  expect(code.newChanges).toEqual({ 2: [{ start: 3, end: 4 }, { start: 8, end: 9 }] });
  expect(code.old[1]).toEqual([{ text: "let  a = 1;" }]);
  expect(code.new[1]).toEqual([{ text: "let a = 2;" }]);
});

test("bundled dark syntax tokenizes both endpoints without substituting the light palette", async () => {
  const code = await highlight({ displayPath: "src/App.tsx", fromContent: "const title = 'old';\n",
    toContent: "const title = 'new';\n", hunks: [] }, "github-dark");
  const oldKeyword = code.old[0].find((token) => token.text === "const")?.color;
  expect(oldKeyword).toBe("#F97583");
  expect(code.new[0].find((token) => token.text === "const")?.color).toBe(oldKeyword);
});

test.each([
  { path: "shader.cpp", prefix: ['const char* shader = R"glsl('], suffix: [')glsl";'] },
  { path: "shader.rb", prefix: ["shader = <<~CPP", 'const char* shader = R"glsl('], suffix: [')glsl";', "CPP"] },
])("embedded shader syntax remains readable in $path", async ({ path, prefix, suffix }) => {
  const shader = ["// shader comment", "uniform vec4 tint;", "void main() { gl_FragColor = tint; }"];
  const lines = [...prefix, ...shader, ...suffix];
  const content = lines.join("\n");
  const code = await highlight({ displayPath: path, fromContent: content, toContent: content, hunks: [] });
  expect(code.old.map((row) => row.map((token) => token.text).join(""))).toEqual(lines);
  expect(code.new.map((row) => row.map((token) => token.text).join(""))).toEqual(lines);
  const declaration = code.new[prefix.length + 1];
  const qualifier = declaration.find((token) => token.text === "uniform")?.color;
  const vector = declaration.find((token) => token.text === "vec4")?.color;
  const comment = code.new[prefix.length][0].color;
  expect(qualifier).toBeDefined();
  expect(vector).toBeDefined();
  expect(qualifier).not.toBe(comment);
  expect(vector).not.toBe(comment);
  expect(code.old[prefix.length + 1]).toEqual(declaration);
});

test("filename lookup favors exact names, then special prefixes, then suffixes", () => {
  expect(languageForPath("config/tsconfig.json")).toBe("jsonc");
  expect(languageForPath("config/other.json")).toBe("json");
  expect(languageForPath("src/Dockerfile.json")).toBe("docker");
  expect(languageForPath("src/Containerfile.ts")).toBe("docker");
  expect(languageForPath("src/Makefile.ts")).toBe("make");
  expect(languageForPath("src/GNUmakefile")).toBe("make");
  expect(languageForPath("src/file.pyi")).toBe("python");
  expect(languageForPath("src/file.unknown.tsx")).toBe("tsx");
  expect(languageForPath("src/file.unknown")).toBeNull();
});

test("unknown extensions do not borrow a directory's grammar", async () => {
  const code = await highlight({
    displayPath: "src/typescript/example.unknown", fromContent: "const before = 1;\n",
    toContent: "const after = 2;\n", hunks: [replacement(1, 1, ["const before = 1;"], ["const after = 2;"])],
  });
  expect(code.old).toEqual([[{ text: "const before = 1;" }]]);
  expect(code.new).toEqual([[{ text: "const after = 2;" }]]);
  expect(code.oldChanges[1]).toEqual([{ start: 6, end: 12 }, { start: 15, end: 16 }]);
  expect(code.newChanges[1]).toEqual([{ start: 6, end: 11 }, { start: 14, end: 15 }]);
});

test("overlong changed lines keep exact text without attempting an expensive inline diff", async () => {
  const removed = "a ".repeat(4096);
  const added = "b ".repeat(4096);
  const code = await highlight({ displayPath: "unknown.bin", fromContent: removed, toContent: added,
    hunks: [replacement(1, 1, [removed], [added])] }, "plain");
  expect(code.oldChanges).toEqual({});
  expect(code.newChanges).toEqual({});
  expect(code.old).toEqual([[{ text: removed }]]);
  expect(code.new).toEqual([[{ text: added }]]);
});

test("dense lines stay readable without corrupting grammar state after long comment closers", async () => {
  const dense = "const x=1;".repeat(200);
  const lines = [dense, "/* opening", `${" ".repeat(20_000)}*/`, "export const visible = 1;"];
  const content = lines.join("\n");
  const code = await highlight({ displayPath: "dense.ts", fromContent: content, toContent: content, hunks: [] });
  expect(code.old[0]).toEqual([{ text: dense }]);
  expect(code.old.map((row) => row.map((token) => token.text).join(""))).toEqual(lines);
  const exported = code.old[3].find((token) => token.text === "export")?.color;
  expect(exported).toBeDefined();
  expect(exported).not.toBe(code.old[1][0].color);
});
