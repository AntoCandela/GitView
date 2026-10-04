/** Exercises subscription retirement so unmounted surfaces cannot receive delayed native invalidations. */
import { beforeEach, expect, test, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { SurfaceNotice } from "../../src/contracts/companion";
import { reviewSurfaceClient } from "../../src/platform/RepositoryClient";
import { deferred } from "../support/deferred";

const channels = vi.hoisted(() => [] as Array<{ onmessage: (notice: unknown) => void }>);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: (notice: unknown) => void = () => {};
    constructor() { channels.push(this); }
  },
}));
beforeEach(() => { channels.length = 0; vi.mocked(invoke).mockReset(); });

test("disposing a surface subscription retires delayed notices without affecting its replacement", async () => {
  vi.mocked(invoke).mockResolvedValue(undefined);
  const oldNotices: SurfaceNotice[] = [];
  const newNotices: SurfaceNotice[] = [];
  const dispose = await reviewSurfaceClient.subscribe(notice => oldNotices.push(notice));
  dispose();
  const stop = await reviewSurfaceClient.subscribe(notice => newNotices.push(notice));
  const notice: SurfaceNotice = { kind: "visibility", visible: false, openEpoch: "closed" };
  channels[0].onmessage(notice);
  channels[1].onmessage(notice);
  expect(oldNotices).toEqual([]);
  expect(newNotices).toEqual([notice]);
  stop();
});

test("a rejected channel registration retires the callback before exposing the failure", async () => {
  const registration = deferred<void>();
  vi.mocked(invoke).mockImplementation(() => registration.promise);
  const notices: SurfaceNotice[] = [];
  const subscription = reviewSurfaceClient.subscribe(notice => notices.push(notice));
  const failed = expect(subscription).rejects.toBe("unavailable");
  registration.reject("unavailable");
  await failed;
  channels[0].onmessage({ kind: "visibility", visible: true, openEpoch: "late" });
  expect(notices).toEqual([]);
});
