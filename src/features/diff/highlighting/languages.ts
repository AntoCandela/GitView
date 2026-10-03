/** Owns worker-only lazy grammars and their filename associations. */

// Only these literal imports enter the bundle; no full registry or runtime CDN loader is used.
export const languages = {
  typescript: () => import("shiki/langs/typescript.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  javascript: () => import("shiki/langs/javascript.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  rust: () => import("shiki/langs/rust.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  jsonc: () => import("shiki/langs/jsonc.mjs"),
  json5: () => import("shiki/langs/json5.mjs"),
  css: () => import("shiki/langs/css.mjs"),
  scss: () => import("shiki/langs/scss.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  vue: () => import("shiki/langs/vue.mjs"),
  svelte: () => import("shiki/langs/svelte.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  shellscript: () => import("shiki/langs/shellscript.mjs"),
  go: () => import("shiki/langs/go.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
  docker: () => import("shiki/langs/docker.mjs"),
  make: () => import("shiki/langs/make.mjs"),
  c: () => import("shiki/langs/c.mjs"),
  cpp: () => import("shiki/langs/cpp.mjs"),
  csharp: () => import("shiki/langs/csharp.mjs"),
  java: () => import("shiki/langs/java.mjs"),
  ruby: () => import("shiki/langs/ruby.mjs"),
  php: () => import("shiki/langs/php.mjs"),
  swift: () => import("shiki/langs/swift.mjs"),
  kotlin: () => import("shiki/langs/kotlin.mjs"),
  xml: () => import("shiki/langs/xml.mjs"),
  ini: () => import("shiki/langs/ini.mjs"),
} satisfies Record<string, () => Promise<{ default: unknown }>>;

export type Language = keyof typeof languages;

// Shiki strips fileTypes. These extend its grammar aliases with filename associations.
const extensions: Record<string, Language> = {
  ts: "typescript", cts: "typescript", mts: "typescript", tsx: "tsx",
  js: "javascript", cjs: "javascript", mjs: "javascript", jsx: "jsx",
  rs: "rust", json: "json", jsonc: "jsonc", json5: "json5", css: "css", scss: "scss",
  html: "html", htm: "html", vue: "vue", svelte: "svelte", md: "markdown", markdown: "markdown",
  yml: "yaml", yaml: "yaml", toml: "toml", py: "python", pyi: "python",
  sh: "shellscript", bash: "shellscript", zsh: "shellscript", go: "go", sql: "sql",
  dockerfile: "docker", mk: "make", makefile: "make", c: "c", h: "c",
  cc: "cpp", cpp: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp", hxx: "cpp",
  cs: "csharp", java: "java", rb: "ruby", rake: "ruby", php: "php", swift: "swift",
  kt: "kotlin", kts: "kotlin", xml: "xml", svg: "xml", ini: "ini", cfg: "ini",
};
const filenames: Record<string, Language> = {
  dockerfile: "docker", containerfile: "docker", makefile: "make", gnumakefile: "make",
  gemfile: "ruby", rakefile: "ruby", ".bashrc": "shellscript", ".zshrc": "shellscript",
  ".bash_profile": "shellscript", ".profile": "shellscript", "tsconfig.json": "jsonc",
  "jsconfig.json": "jsonc",
};
const filenamePrefixes: Record<string, Language> = {
  "dockerfile.": "docker", "containerfile.": "docker", "makefile.": "make",
};

/** Exact filename wins over special prefix, which wins over the longest matching extension. */
export function languageForPath(displayPath: string): Language | null {
  const basename = displayPath.split(/[\\/]/).pop()?.toLowerCase() ?? "";
  if (Object.hasOwn(filenames, basename)) return filenames[basename];
  for (const prefix in filenamePrefixes) {
    if (basename.startsWith(prefix)) return filenamePrefixes[prefix];
  }
  const parts = basename.split(".");
  for (let index = 1; index < parts.length; index++) {
    const suffix = parts.slice(index).join(".");
    if (Object.hasOwn(extensions, suffix)) return extensions[suffix];
  }
  return null;
}
