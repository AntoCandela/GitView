/** Owns bounded JSON-lines transport to a disposable real RepositoryService fixture, never a user's data. */

import { execFileSync, spawn, type ChildProcess, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const target = resolve(root, ".verification/native-target");
const executable = join(target, "debug/examples", process.platform === "win32" ? "journey_bridge.exe" : "journey_bridge");
const requestTimeoutMs = 20_000;
const maxBufferedBytes = 16 * 1024 * 1024;
let build: Promise<void> | undefined;

export interface JourneyFixtureInfo {
  mainLabel: string;
  otherLabel: string;
  mainBranch: string;
  topicBranch: string;
  workingPath: string;
  untrackedPath: string;
  unchangedPath: string;
  headText: string;
  indexText: string;
  workingText: string;
  untrackedText: string;
  unchangedText: string;
  mergeSubject: string;
  rootSubject: string;
  organization?: {
    sourcePath: string;
    headText: string;
    indexText: string;
    workingText: string;
    latePath: string;
    lateText: string;
    rootEntryCount: number;
    shaders: Array<{ path: string; content: string }>;
  };
}

export interface JourneySafety {
  repositoriesIntact: boolean;
  gitStateUnchanged: boolean;
  workingBytesExpected: boolean;
}

/** One Cargo build per module/process; fresh direct Vitest runs need no separate setup command. */
export function buildNativeJourney(): Promise<void> {
  build ??= runNativeJourneyBuild();
  return build;
}

function runNativeJourneyBuild(signal?: AbortSignal): Promise<void> {
  return new Promise<void>((resolveBuild, reject) => {
    signal?.throwIfAborted();
    const child = spawn("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--locked",
      "--target-dir", target, "--example", "journey_bridge"], { cwd: root, stdio: ["ignore", "ignore", "pipe"], detached: process.platform !== "win32" });
    const cancel = () => killProcessTree(child);
    signal?.addEventListener("abort", cancel, { once: true });
    // Cargo diagnostics remain local; error text never publishes paths or repository payloads.
    let outputBytes = 0;
    child.stderr.on("data", (chunk: Buffer) => {
      outputBytes += chunk.length;
      if (outputBytes > maxBufferedBytes) killProcessTree(child);
    });
    const deadline = setTimeout(() => killProcessTree(child), 600_000);
    child.once("error", () => {
      clearTimeout(deadline);
      signal?.removeEventListener("abort", cancel);
      reject(new Error("Native journey build could not start"));
    });
    child.once("close", (code) => {
      clearTimeout(deadline);
      signal?.removeEventListener("abort", cancel);
      if (code === 0 && !signal?.aborted) resolveBuild();
      else reject(new Error("Native journey build failed; run the registered journey_bridge Cargo example build for local diagnostics"));
    });
  });
}

interface PendingReply {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  deadline: NodeJS.Timeout;
}

export class NativeJourney {
  private nextId = 1;
  private pending = new Map<number, PendingReply>();
  private buffer = "";
  private stderrBytes = 0;
  private failure: Error | null = null;
  private closed = false;
  private exited = false;
  private readonly exit: Promise<void>;
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly directory: string;

  private constructor(child: ChildProcessWithoutNullStreams, directory: string) {
    this.child = child;
    this.directory = directory;
    this.exit = new Promise((resolveExit) => {
      child.once("close", () => {
        this.exited = true;
        this.fail(new Error("Native journey process exited"));
        resolveExit();
      });
    });
    child.once("error", () => this.fail(new Error("Native journey process could not start")));
    child.stdin.on("error", () => this.fail(new Error("Native journey input closed")));
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => this.receive(chunk));
    child.stderr.on("data", (chunk: Buffer) => {
      this.stderrBytes += chunk.length;
      if (this.stderrBytes > maxBufferedBytes) this.abort(new Error("Native journey error output exceeded its bound"));
    });
  }

  /** Starts an independent workspace. The signal cancels startup only; callers always own close in finally/afterEach. */
  static async start(mode: "baseline" | "organization" = "baseline", signal?: AbortSignal): Promise<NativeJourney> {
    if (mode !== "baseline" && mode !== "organization") throw new Error("Unsupported native journey fixture mode");
    // Cancellable browser starts own their build; never cancel the shared Vitest build promise.
    await (signal ? runNativeJourneyBuild(signal) : buildNativeJourney());
    signal?.throwIfAborted();
    const directory = await mkdtemp(join(tmpdir(), "gitview-journey-driver-"));
    if (signal?.aborted) {
      await rm(directory, { recursive: true, force: true });
      signal.throwIfAborted();
    }
    const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith("GIT_")));
    environment.GIT_CONFIG_NOSYSTEM = "1";
    environment.GIT_CONFIG_GLOBAL = join(directory, "absent-git-config");
    environment.TMPDIR = directory;
    environment.TMP = directory;
    environment.TEMP = directory;
    const child = spawn(executable, mode === "organization" ? ["--fixture", "organization"] : [],
      { cwd: root, env: environment, stdio: "pipe", detached: process.platform !== "win32" });
    const journey = new NativeJourney(child, directory);
    const cancel = () => journey.abort(new Error("Native journey startup interrupted"));
    signal?.addEventListener("abort", cancel, { once: true });
    try {
      await journey.request<JourneyFixtureInfo>("fixture_info");
      signal?.throwIfAborted();
      return journey;
    } catch (error) {
      await journey.close();
      throw error;
    } finally {
      signal?.removeEventListener("abort", cancel);
    }
  }

  request<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
    if (this.failure || this.closed) return Promise.reject(this.failure ?? new Error("Native journey is closed"));
    if (this.pending.size >= 64) return Promise.reject(new Error("Native journey pending-request bound exceeded"));
    const id = this.nextId++;
    const message = JSON.stringify({ id, command, args }) + "\n";
    if (Buffer.byteLength(message) > 1024 * 1024) return Promise.reject(new Error("Native journey request exceeded its bound"));
    return new Promise<T>((resolveReply, reject) => {
      const deadline = setTimeout(() => this.abort(new Error("Native journey request deadline exceeded")), requestTimeoutMs);
      this.pending.set(id, { resolve: (value) => resolveReply(value as T), reject, deadline });
      this.child.stdin.write(message, (error) => {
        if (error) this.abort(new Error("Native journey request could not be written"));
      });
    });
  }

  /** Requests service shutdown, then reaps the child and removes driver-owned configuration. */
  async close(): Promise<void> {
    if (this.closed) return;
    try {
      if (!this.failure && !this.exited) await this.request("fixture_shutdown");
    } finally {
      this.closed = true;
      this.child.stdin.end();
      const deadline = setTimeout(() => this.kill(), 3_000);
      try { await this.exit; }
      finally {
        clearTimeout(deadline);
        await rm(this.directory, { recursive: true, force: true });
      }
    }
  }

  private receive(chunk: string): void {
    this.buffer += chunk;
    if (Buffer.byteLength(this.buffer) > maxBufferedBytes) {
      this.abort(new Error("Native journey response exceeded its bound"));
      return;
    }
    let newline: number;
    while ((newline = this.buffer.indexOf("\n")) >= 0) {
      const line = this.buffer.slice(0, newline);
      this.buffer = this.buffer.slice(newline + 1);
      try {
        const reply: { id: number; result?: unknown; error?: unknown } = JSON.parse(line);
        const pending = this.pending.get(reply.id);
        if (!pending || !("result" in reply || "error" in reply)) throw new Error("Invalid reply");
        clearTimeout(pending.deadline);
        this.pending.delete(reply.id);
        if ("error" in reply) pending.reject(new Error(typeof reply.error === "string" ? reply.error : "Native journey command failed"));
        else pending.resolve(reply.result);
      } catch {
        this.abort(new Error("Native journey returned an invalid protocol response"));
        return;
      }
    }
  }

  private fail(error: Error): void {
    this.failure ??= error;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.deadline);
      pending.reject(this.failure);
    }
    this.pending.clear();
  }

  private abort(error: Error): void {
    this.fail(error);
    this.kill();
  }

  private kill(): void {
    if (this.exited) return;
    killProcessTree(this.child);
  }
}

/** Timeout cleanup includes Git/compiler descendants, not just their immediate parent. */
function killProcessTree(child: ChildProcess): void {
  if (!child.pid) return;
  try {
    if (process.platform === "win32") {
      execFileSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore", timeout: 3_000 });
    } else process.kill(-child.pid, "SIGKILL");
  } catch {
    // The process may already have exited; direct termination is the remaining fallback.
    child.kill("SIGKILL");
  }
}
