/** Protects association precedence and safe fallback for arbitrary repository display names. */
import { describe, expect, it } from "vitest";
import { fileIcon, folderIcon } from "../../src/ui/file-icons/fileIconLookup";

describe.each(["material", "catppuccin"] as const)("%s icon lookup", (theme) => {
  it("prefers a recognized filename over its generic extension", () => {
    expect(fileIcon(theme, "tsconfig.json")).not.toBe(fileIcon(theme, "unknown.json"));
    expect(fileIcon(theme, "TSCONFIG.JSON")).toBe(fileIcon(theme, "tsconfig.json"));
  });
  it("prefers compound extensions over the final extension", () => {
    expect(fileIcon(theme, "widget.test.ts")).not.toBe(fileIcon(theme, "widget.ts"));
    expect(fileIcon(theme, "widget.d.ts")).not.toBe(fileIcon(theme, "widget.ts"));
    expect(fileIcon(theme, "widget.part.test.ts")).toBe(fileIcon(theme, "widget.test.ts"));
  });
  it("falls back safely for extensionless, unknown and object-prototype names", () => {
    expect(fileIcon(theme, "constructor")).toBe(fileIcon(theme, "unknown-extensionless"));
    expect(fileIcon(theme, "__proto__")).toBe(fileIcon(theme, "unknown-extensionless"));
    expect(fileIcon(theme, "file.constructor")).toBe(fileIcon(theme, "file.unrecognized-extension"));
  });
  it("distinguishes named folders and expanded artwork without losing fallback", () => {
    expect(folderIcon(theme, "SRC", false)).toBe(folderIcon(theme, "src", false));
    expect(folderIcon(theme, "src", true)).not.toBe(folderIcon(theme, "src", false));
    expect(folderIcon(theme, "tests", false)).not.toBe(folderIcon(theme, "src", false));
    expect(folderIcon(theme, "constructor", true)).toBe(folderIcon(theme, "unknown-folder", true));
    expect(folderIcon(theme, "unknown-folder", true)).not.toBe(folderIcon(theme, "unknown-folder", false));
  });
});

it("recognizes upstream Material associations that contain mixed-case keys", () => {
  expect(fileIcon("material", "APKBUILD")).not.toBe(fileIcon("material", "unknown-extensionless"));
  expect(fileIcon("material", "apkbuild")).toBe(fileIcon("material", "APKBUILD"));
  expect(fileIcon("material", "syntax.YAML-tmLanguage")).not.toBe(fileIcon("material", "syntax.unknown-extension"));
  expect(folderIcon("material", "iPhone", false)).not.toBe(folderIcon("material", "unknown-folder", false));
  expect(folderIcon("material", "IPHONE", true)).toBe(folderIcon("material", "iPhone", true));
});
