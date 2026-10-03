# GitView branding

**Selected identity: View V (04).** The app uses its framed V-shaped commit path, the browser uses its adaptive favicon, and the five configured native icons are exported from its Cream/navy/teal SVG master.

Open [the visual comparison](branding.html) through the existing Vite server (`npm run dev`, then `/docs/branding.html`). It uses the repository's self-hosted Geist font and shows color lockups, app-icon mockups, reversed marks and 16/24/32px checks.

[View the two additional variants](branding-new-variants.png) for a static side-by-side comparison of View V and Parallel V.

## The five directions

1. **Branch Frame** (`branch-frame`) — Git ancestry inside a rounded, open G-shaped view frame.
2. **Split View** (`split-view`) — paired source panes with commit nodes. The quietest option and closest visual metaphor for side-by-side comparison.
3. **Ancestry V** (`ancestry-v`) — a V-shaped silhouette with a branching history path. The boldest option, suited to a standalone app mark.
4. **View V** (`view-v`) — a V-shaped commit path within a rounded view frame. Combines the two preferred directions in a compact app mark.
5. **Parallel V** (`parallel-v`) — paired angled source panes surround a smaller V and connected commit path. More open and comparison-led.

All five use the existing palette, not a new interface theme:

| Role | Color |
| --- | --- |
| Navy ink | `#020d26` |
| Teal accent | `#0f766e` |
| Cream / reversed artwork | `#f8f4ec` |

## Files

Candidate SVGs are in `src/assets/brand/`, with this naming pattern:

`{concept}-{icon|horizontal|vertical}-{color|dark|light}.svg`

The five documented concepts each have three layouts and three color treatments. `dark` means navy monochrome for light backgrounds; `light` means cream monochrome for dark backgrounds. Only View V is imported into the app; alternative concepts are retained as design history and are not bundled.

`src/ui/Brand.tsx` uses `view-v-symbol.svg` for geometry, with frame and branch colors from the interface's ink/accent tokens. The active workbench has one 24px toolbar mark; the empty state has the full lockup with the existing self-hosted Geist wordmark. `public/favicon.svg` adapts to browser light/dark appearance. `src/assets/brand/app-icon.svg` is the fixed Cream/navy/teal native master.

Regenerate the SVGs and comparison page:

```sh
node scripts/generate-brand.mjs
```

Regenerate native exports with the locked Tauri CLI, then install only the five files configured in `src-tauri/tauri.conf.json`:

```sh
npm run tauri -- icon src/assets/brand/app-icon.svg --output .verification/view-v-icons
cp .verification/view-v-icons/{32x32.png,128x128.png,128x128@2x.png,icon.icns,icon.ico} src-tauri/icons/
```

For a macOS `.app` developer-preview bundle, run `npm run tauri -- build --bundles app`. Quit the old GitView process before opening the rebuilt `src-tauri/target/release/bundle/macos/GitView.app`. Updating source icons does not update an already-running app or another installed copy; an existing Dock pin can still point to that older copy. Do not treat icon replacement or successful packaging as application/release certification.

After intentional artwork changes, refresh the repository's existing license inventory with `node scripts/check-licenses.mjs --refresh`; `--inventory-only` checks integrity without claiming publication clearance.

Geometry is original project artwork, generated for this repository rather than copied from an upstream logo or icon library. It is contributed under the repository's GPL-3.0-only terms. This is not a trademark-clearance assessment. Geist remains separately licensed under OFL-1.1; existing font attribution is in `THIRD_PARTY_NOTICES.md`.

## Usage and technical limits

- Preserve the SVG viewBox and aspect ratio; do not stretch, rotate or add shadows.
- Leave at least 8 units of clear space around a 64-unit icon. Use one identity in each surface, not repeated decorative workbench labels.
- Prefer icon-only at compact sizes. The comparison includes 16px as a stress check; 24px is the recommended in-app minimum, with 32px preferred for standalone color icons.
- Use the shared theme-aware primitive in-app, or the monochrome reversed export on dark external backgrounds. Do not put navy artwork directly on Midnight.
- Horizontal and stacked SVG wordmarks contain live text with `Geist Variable, Geist, sans-serif`. The comparison embeds them inline so the bundled font applies. A standalone SVG viewed without Geist can use a fallback; outline or embed the licensed font before final external/export delivery.
- App-icon tiles are mockups, not native exports or evidence of packaged-app appearance.
- The unselected concepts are documentation-only. Native exports and a browser screenshot do not certify packaged macOS/Windows/Linux appearance; signing, notarization and installer verification remain separate.

## Verified branding boundary

The macOS arm64 developer-preview `.app` was produced with `npm run tauri -- build --bundles app`. Its `CFBundleIconFile` points to `icon.icns`; the bundled resource matched `src-tauri/icons/icon.icns` byte-for-byte and decoded to the View V artwork.

Chromium exercised the shared lockup and compact mark with light/dark interface palettes. At 320px, a long repository name, the sidebar toggle and both appearance/layout actions fit the 36px toolbar without overlap; the repository disclosure stayed inside the viewport. The production renderer served the external SVG symbol and favicon successfully. Favicon light/dark media queries selected the expected navy/teal and cream/bright-teal colors.

These are artwork, browser-renderer and packaged-resource observations, not an observed Dock refresh, native-picker test, installer test or release certification. The user’s older running/installed app was not stopped or overwritten.
