/** Replaces Shiki's unlicensed upstream GLSL asset before app, worker or test bundling. */
import { fileURLToPath } from "node:url";
import { normalizePath, type Plugin } from "vite";

const upstreamGrammar = normalizePath(fileURLToPath(new URL("../node_modules/@shikijs/langs/dist/glsl.mjs", import.meta.url)));
const licensedGrammar = normalizePath(fileURLToPath(new URL("../src/assets/grammars/glsl.json", import.meta.url)));

/** Keep the original module identity so C++/Ruby imports receive the licensed GLSL grammar. */
export function glslGrammar(): Plugin {
  return {
    name: "gitview-licensed-glsl",
    enforce: "pre",
    load(id) {
      if (normalizePath(id.split("?")[0]) !== upstreamGrammar) return null;
      return `import grammar from ${JSON.stringify(licensedGrammar)};\nexport default [grammar];\n`;
    },
  };
}
