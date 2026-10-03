/** Exercises built renderer assets against a page-owned real service; reports only bounded synthetic evidence. */

import assert from "node:assert/strict";
import { fork } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, expect as playwrightExpect } from "@playwright/test";
import { NativeJourney } from "../tests/support/nativeJourney.ts";
import { installBrowserJourney } from "../tests/support/browserJourney.ts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(root, ".verification/browser-smoke");
const argumentsList = process.argv.slice(2);
const scenario = argumentsList.length === 2 && argumentsList[0] === "--scenario"
  && ["baseline", "organization"].includes(argumentsList[1]) ? argumentsList[1] : null;
const timeout = 20_000;
const expect = playwrightExpect.configure({ timeout });

async function expandPath(scope, path) {
  const segments = path.split("/");
  for (let depth = 1; depth < segments.length; depth++) {
    const directory = segments.slice(0, depth).join("/");
    const collapsed = scope.getByRole("button", { name: `Expand ${directory}`, exact: true });
    const expanded = scope.getByRole("button", { name: `Collapse ${directory}`, exact: true });
    await expect(collapsed.or(expanded)).toBeVisible({ timeout });
    if (await collapsed.count()) await collapsed.click();
  }
}

function source(page, side = "New") {
  return page.getByRole("region", { name: new RegExp(`^${side} source (file|hunks)$`) });
}

async function expectSource(page, text, side = "New") {
  const lines = text.replace(/\n$/, "").split("\n");
  await expect.poll(() => source(page, side).locator("code").allTextContents(), { timeout }).toEqual(lines);
}

async function openMain(page, fixture) {
  await page.getByRole("button", { name: /^Current repository:/ }).click();
  await page.getByRole("button", { name: "Open repository", exact: true }).click();
  const selector = page.getByRole("button", { name: /^Current repository:/ });
  if (await selector.getAttribute("aria-expanded") !== "true") await selector.click();
  await page.getByRole("button", { name: `${fixture.mainLabel} ${fixture.mainBranch}`, exact: true }).click();
  await expect(page.getByRole("button", { name: `Current repository: ${fixture.mainLabel}`, exact: true })).toBeVisible();
}

async function reviewChanged(page, path) {
  const files = page.getByRole("complementary", { name: "Files", exact: true });
  await expandPath(files, path);
  await files.getByRole("button", { name: `Review ${path}`, exact: true }).click();
}

async function baseline(page, fixture) {
  await openMain(page, fixture);
  await reviewChanged(page, fixture.workingPath);
  await expectSource(page, fixture.workingText);
  await expectSource(page, fixture.indexText, "Old");
}

async function observeHighlighting(page, assets, workers, theme) {
  await expect.poll(() => workers.has("highlighting.worker"), { timeout }).toBe(true);
  for (const asset of ["highlighting.worker", "typescript", "wasm", theme]) {
    await expect.poll(() => assets.has(asset), { timeout }).toBe(true);
  }
  await expect.poll(() => source(page).locator("code span[style]").evaluateAll((spans) =>
    new Set(spans.map((span) => span.style.color).filter(Boolean)).size), { timeout }).toBeGreaterThan(1);
  await expect(page.getByText("Syntax highlighting unavailable; source remains readable.", { exact: true })).toHaveCount(0);
}

async function changePreferences(page, fixture, assets, workers) {
  await page.getByRole("radio", { name: "Full file", exact: true }).check();
  await page.getByRole("radio", { name: "Wrap", exact: true }).check();
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  await page.getByRole("button", { name: "Midnight", exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(source(page)).toHaveAccessibleName("New source file");
  await expect(page.locator(".diff-viewport")).toHaveClass(/is-wrapped/);
  await expect.poll(() => page.evaluate(() => localStorage.getItem("gitview.app-theme"))).toBe("midnight");
  await expectSource(page, fixture.organization.workingText);
  await observeHighlighting(page, assets, workers, "github-dark");
}

async function preserveLayout(page, fixture) {
  const pane = await source(page).elementHandle();
  assert(pane);
  const divider = page.getByRole("separator", { name: "Resize old and new versions", exact: true });
  const initial = await divider.getAttribute("aria-valuenow");
  await divider.press("ArrowRight");
  await expect(divider).not.toHaveAttribute("aria-valuenow", initial);
  const ratio = await divider.getAttribute("aria-valuetext");
  const row = page.getByRole("separator", { name: "Resize panel rows", exact: true });
  const height = await row.getAttribute("aria-valuenow");
  await row.press("ArrowDown");
  await expect(row).not.toHaveAttribute("aria-valuenow", height);
  await page.getByRole("button", { name: "Workbench layout", exact: true }).click();
  await page.getByRole("radio", { name: "Graph above, Files bottom left, Preview bottom right", exact: true }).check();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("region", { name: "Repository workbench", exact: true }))
    .toHaveAttribute("data-workbench-layout", "history-files-comparison");
  await page.setViewportSize({ width: 820, height: 720 });
  await expectSource(page, fixture.organization.workingText);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await expect(divider).toHaveAttribute("aria-valuetext", ratio);
  assert(await pane.evaluate((element) => element.isConnected), "Preview remounted during layout changes");
  await expect(page.getByRole("radio", { name: "Wrap", exact: true })).toBeChecked();
  await expect(page.getByRole("radio", { name: "Full file", exact: true })).toBeChecked();
  await pane.dispose();
}

async function exerciseFileControls(page, fixture) {
  const files = page.getByRole("complementary", { name: "Files", exact: true });
  await files.getByRole("button", { name: "Expand all", exact: true }).click();
  await expect(files.getByRole("button", { name: "Collapse all", exact: true })).toBeEnabled();
  await files.getByRole("button", { name: "Collapse all", exact: true }).click();
  await files.getByRole("button", { name: "Switch to list view", exact: true }).click();
  await expect(files.getByRole("list", { name: "Changed file list", exact: true })).toBeVisible();
  await expectSource(page, fixture.organization.workingText);
  await files.getByRole("button", { name: "Switch to tree view", exact: true }).press("Enter");
  await expandPath(files, fixture.organization.sourcePath);
  await expect(files.getByRole("button", { name: `Review ${fixture.organization.sourcePath}`, exact: true }))
    .toHaveAttribute("aria-pressed", "true");
}

async function committedReview(page, fixture) {
  await page.getByRole("button", { name: new RegExp(`^${fixture.mergeSubject}, Commit `) }).click();
  const files = page.getByRole("region", { name: /^Changed files for commit / });
  await expandPath(files, "src/topic.txt");
  await files.getByRole("button", { name: "Review src/topic.txt", exact: true }).click();
  await expectSource(page, "topic contribution");
  await expect(source(page)).not.toContainText("working organization");
  await reviewChanged(page, fixture.organization.sourcePath);
  await expectSource(page, fixture.organization.workingText);
}

async function browseAndRefresh(page, fixture, observations) {
  await page.getByRole("button", { name: "Expand sidebar", exact: true }).click();
  const sidebar = page.getByRole("complementary", { name: "Workspace sidebar", exact: true });
  await expandPath(sidebar, fixture.unchangedPath);
  await sidebar.getByRole("button", { name: `Review ${fixture.unchangedPath}`, exact: true }).click();
  await expectSource(page, fixture.unchangedText);
  const tree = sidebar.locator(".changes-tree");
  const late = sidebar.getByRole("button", { name: `Review ${fixture.organization.latePath}`, exact: true });
  await expect.poll(async () => {
    await tree.evaluate((element) => { element.scrollTop = element.scrollHeight; });
    return late.isVisible();
  }, { timeout }).toBe(true);
  await late.click();
  await expectSource(page, fixture.organization.lateText);
  const top = await tree.evaluate((element) => element.scrollTop);
  assert(top > 768 * 20, "Fixture did not reach a late virtualized page");
  assert(await tree.locator("[data-tree-index]").count() < 100, "Tree mounted unbounded rows");
  const completed = observations.rootCompletions;
  await sidebar.getByRole("button", { name: "Refresh files", exact: true }).click();
  await expect.poll(() => observations.rootCompletions, { timeout }).toBeGreaterThan(completed);
  const replacement = observations.completedRootListingId;
  await expect.poll(async () => {
    // Do not let Playwright scroll a lost anchor back into view while probing the cutover.
    if (Math.abs(await tree.evaluate((element) => element.scrollTop) - top) >= 1) return null;
    await late.click();
    return observations.reviewListingId;
  }, { timeout }).toBe(replacement);
  await expect(sidebar.getByRole("alert")).toHaveCount(0);
  await expect.poll(() => tree.evaluate((element) => element.scrollTop), { timeout }).toBe(top);
  await expect(late).toHaveAttribute("aria-pressed", "true");
  await expectSource(page, fixture.organization.lateText);
  await expect(page.getByRole("radio", { name: "Wrap", exact: true })).toBeChecked();
  await sidebar.getByRole("button", { name: "Switch to list view", exact: true }).click();
  await expect(sidebar.getByRole("list", { name: "Repository file list", exact: true })).toBeVisible();
  await sidebar.getByRole("button", { name: "Switch to tree view", exact: true }).press("Space");
  await expect(sidebar.getByRole("list", { name: "Repository file hierarchy", exact: true })).toBeVisible();
}

async function main() {
  if (!scenario) {
    console.error("Usage: npm run smoke:browser -- --scenario baseline|organization");
    process.exitCode = 2;
    return;
  }
  await mkdir(output, { recursive: true });
  const report = { scenario, status: "failed", phase: "setup", completed: [], fixtureSafe: false,
    cleanup: { browser: true, native: true, preview: true }, boundary: "built-renderer-real-service-not-native-host" };
  let server;
  let serverExit;
  let browser;
  let journey;
  let page;
  const controller = new AbortController();
  const signalHandler = () => controller.abort(new Error("Browser smoke interrupted"));
  process.on("SIGINT", signalHandler);
  process.on("SIGTERM", signalHandler);
  let deadline = setTimeout(() => controller.abort(new Error("Browser smoke setup deadline exceeded")), 780_000);
  async function step(name, action) {
    report.phase = name;
    await action();
    report.completed.push(name);
  }
  try {
    assert((await stat(resolve(root, "dist/index.html"))).isFile(), "Build frontend before browser smoke");
    controller.signal.throwIfAborted();
    server = fork(resolve(root, "tests/support/browserPreview.mjs"), [], {
      cwd: root, stdio: ["ignore", "ignore", "ignore", "ipc"],
    });
    serverExit = new Promise((resolveExit) => server.once("close", resolveExit));
    controller.signal.addEventListener("abort", () => server.kill("SIGTERM"), { once: true });
    report.cleanup.preview = false;
    let previewDeadline;
    const port = await Promise.race([
      new Promise((resolvePort, reject) => {
        server.once("message", (message) => {
          if (Number.isInteger(message?.port) && message.port > 0 && message.port <= 65535) resolvePort(message.port);
          else reject(new Error("Browser preview returned invalid readiness"));
        });
        server.once("error", () => reject(new Error("Browser preview could not start")));
        previewDeadline = setTimeout(() => reject(new Error("Browser preview startup deadline exceeded")), 20_000);
      }),
      serverExit.then(() => { throw new Error("Browser preview exited during startup"); }),
    ]).finally(() => clearTimeout(previewDeadline));
    controller.signal.throwIfAborted();
    journey = await NativeJourney.start(scenario, controller.signal);
    report.cleanup.native = false;
    controller.signal.throwIfAborted();
    const fixture = await journey.request("fixture_info");
    browser = await chromium.launch({ headless: true, timeout: 30_000 });
    report.cleanup.browser = false;
    controller.signal.throwIfAborted();
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    page = await context.newPage();
    page.setDefaultTimeout(timeout);
    const assets = new Set();
    const workers = new Set();
    const observations = { rootCompletions: 0, completedRootListingId: null, reviewListingId: null };
    let pageErrors = 0;
    page.on("pageerror", () => { pageErrors++; });
    page.on("worker", (worker) => { if (/\/highlighting\.worker-[^/]+\.js$/.test(worker.url())) workers.add("highlighting.worker"); });
    context.on("response", (response) => {
      if (response.status() !== 200) return;
      const match = new URL(response.url()).pathname.match(/\/(highlighting\.worker|typescript|wasm|github-light|github-dark)-[^/]+\.js$/);
      if (match) assets.add(match[1]);
    });
    await installBrowserJourney({
      exposeFunction: (name, callback) => page.exposeFunction(name, async (command, args) => {
        const result = await callback(command, args);
        if (command === "list_repository_files" && result.kind === "files" && result.directoryId === null && result.cursor === null) {
          observations.rootCompletions++;
          observations.completedRootListingId = result.listingId;
        }
        if (command === "review_repository_file" && result.kind === "text") observations.reviewListingId = result.listingId;
        return result;
      }),
      addInitScript: (script) => page.addInitScript(script),
    }, journey);
    controller.signal.throwIfAborted();
    clearTimeout(deadline);
    deadline = setTimeout(() => controller.abort(new Error("Browser smoke deadline exceeded")), 180_000);
    const interrupted = new Promise((_, reject) => {
      controller.signal.addEventListener("abort", () => reject(controller.signal.reason), { once: true });
    });
    await Promise.race([interrupted, (async () => {
      await step("baseline-working-text", async () => {
        await page.goto(`http://127.0.0.1:${port}`, { waitUntil: "load" });
        await baseline(page, fixture);
      });
      if (scenario === "organization") {
        assert(fixture.organization?.rootEntryCount >= 768);
        await step("working-staged-endpoints", async () => {
          await reviewChanged(page, fixture.organization.sourcePath);
          await expectSource(page, fixture.organization.workingText);
          await expectSource(page, fixture.organization.indexText, "Old");
          await page.getByRole("button", { name: "Staged comparison", exact: true }).click();
          await expectSource(page, fixture.organization.indexText);
          await expectSource(page, fixture.organization.headText, "Old");
          await page.getByRole("button", { name: "Unstaged comparison", exact: true }).click();
          await expectSource(page, fixture.organization.workingText);
        });
        await step("worker-assets-inert-source", async () => {
          await observeHighlighting(page, assets, workers, "github-light");
          assert.equal(await source(page).locator("img, script, iframe").count(), 0);
          assert.equal(await page.evaluate(() => globalThis.journeyInjected), undefined);
        });
        await step("licensed-embedded-shaders", async () => {
          for (const shader of fixture.organization.shaders) {
            await reviewChanged(page, shader.path);
            await expectSource(page, shader.content);
            const qualifier = source(page).getByText("uniform", { exact: true });
            const vector = source(page).getByText("vec4", { exact: true });
            const comment = source(page).getByText("// shader comment", { exact: true });
            await expect(qualifier).toBeVisible();
            await expect(vector).toBeVisible();
            await expect.poll(async () => {
              const colors = await Promise.all([qualifier, vector, comment].map((token) =>
                token.evaluate((element) => getComputedStyle(element).color)));
              return colors[0] !== colors[2] && colors[1] !== colors[2];
            }).toBe(true);
            await page.screenshot({ path: resolve(output, shader.path.endsWith(".cpp") ? "shader-cpp.png" : "shader-ruby.png"), fullPage: true });
          }
          await reviewChanged(page, fixture.organization.sourcePath);
          await expectSource(page, fixture.organization.workingText);
        });
        await step("preferences-worker-theme", () => changePreferences(page, fixture, assets, workers));
        await step("viewport-layout-divider-continuity", () => preserveLayout(page, fixture));
        await step("tree-list-folder-controls", () => exerciseFileControls(page, fixture));
        await step("committed-working-continuity", () => committedReview(page, fixture));
        await step("browsed-refresh-anchor", () => browseAndRefresh(page, fixture, observations));
      }
      await step("renderer-error-free", async () => assert.equal(pageErrors, 0));
      await page.screenshot({ path: resolve(output, `${scenario}.png`), fullPage: true, timeout: 10_000 });
    })()]);
    report.status = "passed";
  } catch {
    // Raw browser/native errors can contain paths or source. Only the fixed failing phase is public.
    report.status = "failed";
  } finally {
    clearTimeout(deadline);
    if (journey) {
      try {
        const safety = await journey.request("fixture_verify");
        assert.deepEqual(safety, { repositoriesIntact: true, gitStateUnchanged: true, workingBytesExpected: true });
        report.fixtureSafe = true;
      } catch { report.status = "failed"; }
    }
    if (browser) {
      try { await browser.close(); report.cleanup.browser = !browser.isConnected(); }
      catch { report.status = "failed"; }
    }
    if (journey) {
      try { await journey.close(); report.cleanup.native = true; }
      catch { report.status = "failed"; }
    }
    if (server) {
      const force = setTimeout(() => server.kill("SIGKILL"), 3_000);
      try {
        server.kill("SIGTERM");
        await serverExit;
        report.cleanup.preview = true;
      } catch { report.status = "failed"; }
      finally { clearTimeout(force); }
    }
    process.removeListener("SIGINT", signalHandler);
    process.removeListener("SIGTERM", signalHandler);
    if (!report.fixtureSafe || Object.values(report.cleanup).some((closed) => !closed)) report.status = "failed";
    await writeFile(resolve(output, `${scenario}.json`), `${JSON.stringify(report, null, 2)}\n`);
    console.log(JSON.stringify(report));
    process.exitCode = report.status === "passed" ? 0 : 1;
  }
}

await main().catch(() => {
  console.error("Browser smoke could not persist its sanitized summary");
  process.exitCode = 1;
});
