/** Generates View V runtime masters and retains the original logo exploration for documentation. */
import { mkdir, readFile, writeFile } from "node:fs/promises";

const concepts = {
  "branch-frame": (ink, accent) => `<path d="M43 12H24a12 12 0 0 0-12 12v16a12 12 0 0 0 12 12h16a12 12 0 0 0 12-12v-8H36" fill="none" stroke="${ink}" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/><path d="M26 22v13c0 5 4 9 9 9h7M26 34c0-9 14-6 14-15" fill="none" stroke="${accent}" stroke-width="4" stroke-linecap="round"/><g fill="${accent}"><circle cx="26" cy="22" r="4"/><circle cx="40" cy="18" r="4"/></g>`,
  "split-view": (ink, accent) => `<rect x="10" y="12" width="44" height="40" rx="10" fill="none" stroke="${ink}" stroke-width="5"/><path d="M32 14v36M20 24v16M44 24v16" stroke="${accent}" stroke-width="4" stroke-linecap="round"/><circle cx="20" cy="24" r="4" fill="${accent}"/><circle cx="44" cy="40" r="4" fill="${accent}"/>`,
  "ancestry-v": (ink, accent) => `<path d="m12 16 20 36 20-36" fill="none" stroke="${ink}" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/><path d="M32 50V30c0-8 12-6 12-14" fill="none" stroke="${accent}" stroke-width="4" stroke-linecap="round"/><circle cx="12" cy="16" r="5" fill="${ink}"/><circle cx="44" cy="16" r="5" fill="${accent}"/><circle cx="32" cy="50" r="5" fill="${accent}"/>`,
  "view-v": (ink, accent) => `<rect x="10" y="10" width="44" height="44" rx="10" fill="none" stroke="${ink}" stroke-width="5"/><path d="M22 21v8l10 14 10-14v-8" fill="none" stroke="${accent}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/><g fill="${accent}"><circle cx="22" cy="21" r="4"/><circle cx="42" cy="21" r="4"/><circle cx="32" cy="43" r="4"/></g>`,
  "parallel-v": (ink, accent) => `<path d="m12 16 14 34M52 16 38 50" fill="none" stroke="${ink}" stroke-width="6" stroke-linecap="round"/><path d="m25 18 7 18 7-18M26 50c6 0 8-6 12-6" fill="none" stroke="${accent}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/><g fill="${accent}"><circle cx="25" cy="18" r="4"/><circle cx="39" cy="18" r="4"/><circle cx="26" cy="50" r="4"/></g>`,
};
const palettes = { color: ["#020d26", "#0f766e"], dark: ["#020d26", "#020d26"], light: ["#f8f4ec", "#f8f4ec"] };
const layouts = { icon: [64, 64], horizontal: [248, 64], vertical: [160, 120] };
const root = new URL("../src/assets/brand/", import.meta.url);
await mkdir(root, { recursive: true });
for (const [concept, draw] of Object.entries(concepts)) {
  for (const [palette, [ink, accent]] of Object.entries(palettes)) {
    for (const [layout, [width, height]] of Object.entries(layouts)) {
      const symbol = layout === "vertical" ? `<g transform="translate(48 4)">${draw(ink, accent)}</g>` : draw(ink, accent);
      const wordmark = layout === "icon" ? "" : `<text x="${layout === "vertical" ? 80 : 76}" y="${layout === "vertical" ? 102 : 43}" ${layout === "vertical" ? 'text-anchor="middle" ' : ""}font-family="Geist Variable, Geist, sans-serif" font-size="${layout === "vertical" ? 30 : 36}" font-weight="650" letter-spacing="-1.3" fill="${ink}">GitView</text>`;
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title desc"><title id="title">GitView</title><desc id="desc">${concept.replaceAll("-", " ")} logo concept for local Git inspection</desc>${symbol}${wordmark}</svg>\n`;
      await writeFile(new URL(`${concept}-${layout}-${palette}.svg`, root), svg);
    }
  }
}
const descriptions = {
  "branch-frame": ["01", "Branch Frame", "A branching G inside an open view frame. The most distinctive connection between Git and GitView."],
  "split-view": ["02", "Split View", "Two source panes in one rounded frame. Quiet, precise and closest to the side-by-side workbench."],
  "ancestry-v": ["03", "Ancestry V", "A bold V with a connected history path. More expressive, with a strong app-icon silhouette."],
  "view-v": ["04", "View V", "A V-shaped commit path inside the split-view frame. Combines your two preferred directions in one compact mark."],
  "parallel-v": ["05", "Parallel V", "Paired angled source panes form a V around a smaller branching path. More open, geometric and comparison-led."],
};
const panels = [];
for (const [concept, [number, name, description]] of Object.entries(descriptions)) {
  const horizontal = await readFile(new URL(`${concept}-horizontal-color.svg`, root), "utf8");
  panels.push(`<article><header><span>${number}</span><h2>${name}</h2></header><div class="hero">${horizontal}</div><p>${description}</p><div class="samples"><div class="tile"><svg viewBox="0 0 64 64" role="img" aria-label="${name} app icon">${concepts[concept]("#020d26", "#0f766e")}</svg></div><div class="small"><span>Small-size check</span><div>${[16, 24, 32].map(size => `<svg viewBox="0 0 64 64" width="${size}" height="${size}" role="img" aria-label="${size} pixel ${name}">${concepts[concept]("#020d26", "#020d26")}</svg>`).join("")}</div></div></div><div class="reversed"><svg viewBox="0 0 64 64" role="img" aria-label="Reversed ${name}">${concepts[concept]("#f8f4ec", "#f8f4ec")}</svg><span>GitView</span></div><footer><a href="../src/assets/brand/${concept}-horizontal-color.svg">Horizontal SVG</a><a href="../src/assets/brand/${concept}-icon-color.svg">Icon SVG</a></footer></article>`);
}
await writeFile(new URL("../docs/branding.html", import.meta.url), `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>GitView — five brand directions</title><style>
@font-face{font-family:"Geist Variable";src:url("../node_modules/@fontsource-variable/geist/files/geist-latin-wght-normal.woff2") format("woff2");font-weight:100 900}
*{box-sizing:border-box}body{margin:0;background:#f8f4ec;color:#020d26;font-family:"Geist Variable",sans-serif;padding:48px}main{max-width:1200px;margin:auto}.intro{display:flex;justify-content:space-between;gap:24px;align-items:end;margin-bottom:40px}.eyebrow{font-size:12px;letter-spacing:2px;text-transform:uppercase;color:#0f766e}h1{font-size:40px;letter-spacing:-1.6px;font-weight:650;margin:12px 0}.intro p{max-width:320px;font-size:14px;line-height:1.6;color:#596071}.grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:24px}article{min-width:0;border-top:1px solid #cdc5b8;padding-top:20px}article header{display:flex;gap:12px;align-items:center}header span{font-size:12px;color:#0f766e}h2{font-size:17px;font-weight:600;margin:0}.hero{display:flex;align-items:center;justify-content:center;height:180px}.hero svg{width:100%;max-width:280px}article p{font-size:13px;color:#596071;line-height:1.7;min-height:68px}.samples{display:flex;align-items:center;gap:24px;margin:24px 0}.tile{width:80px;height:80px;border-radius:18px;background:#ede8dd;display:grid;place-items:center}.tile svg{width:60px;height:60px}.small{font-size:11px;color:#596071}.small div{display:flex;align-items:center;gap:16px;margin-top:12px}.reversed{display:flex;align-items:center;justify-content:center;gap:12px;height:100px;background:#020d26;border-radius:8px;color:#f8f4ec}.reversed svg{width:40px;height:40px}.reversed span{font-size:26px;font-weight:650;letter-spacing:-1px}footer{display:flex;gap:20px;margin-top:20px}a{font-size:12px;color:#0f766e;text-underline-offset:4px}.note{margin-top:36px;font-size:12px;color:#596071}@media(max-width:800px){body{padding:24px}.grid{grid-template-columns:1fr}.intro{display:block}.hero{height:140px}article p{min-height:0}h1{font-size:32px}}
</style></head><body><main><div class="intro"><div><span class="eyebrow">GitView / Brand exploration</span><h1>Your repository, in view.</h1></div><p>View V (04) is the selected identity.<br>The other directions are retained here as design exploration, not runtime imports.</p></div><div class="grid">${panels.join("")}</div><p class="note">View V is used by the app, favicon and native icon bundle. Each direction includes horizontal, stacked and icon-only SVGs, in full color, navy and reversed cream. Tiles on this page are mockups, not evidence of packaged native applications.</p></main></body></html>`);
const selectedMark = concepts["view-v"]("#020d26", "#0f766e");
const frame = selectedMark.match(/<rect[^>]+\/>/)[0];
const branch = selectedMark.slice(frame.length);
await writeFile(new URL("view-v-symbol.svg", root), `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><g id="frame">${frame.replaceAll("#020d26", "currentColor")}</g><g id="branch">${branch.replaceAll("#0f766e", "currentColor")}</g></svg>\n`);
await writeFile(new URL("app-icon.svg", root), `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><title>GitView — View V</title><rect x="64" y="64" width="896" height="896" rx="208" fill="#f8f4ec"/><g transform="translate(160 160) scale(11)">${selectedMark}</g></svg>\n`);
await mkdir(new URL("../public/", import.meta.url), { recursive: true });
await writeFile(new URL("../public/favicon.svg", import.meta.url), `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><title>GitView</title><style>@media(prefers-color-scheme:dark){.frame{stroke:#f8f4ec}.branch{stroke:#2dd4bf}.nodes{fill:#2dd4bf}}</style>${frame.replace('stroke="#020d26"', 'class="frame" stroke="#020d26"')}${branch.replace('stroke="#0f766e"', 'class="branch" stroke="#0f766e"').replace('fill="#0f766e"', 'class="nodes" fill="#0f766e"')}</svg>\n`);
console.log("Generated View V symbol, adaptive favicon and native icon master, plus the documented concept exploration.");
