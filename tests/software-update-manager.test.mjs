import assert from "node:assert/strict";
import test from "node:test";
import { createSoftwareUpdateManager, UPDATE_CHECK_INTERVAL_MS } from "../src/software-update-manager.ts";

function fakeUpdate(version) {
  const calls = { close: 0, download: 0, install: 0 };
  return {
    version,
    calls,
    async close() { calls.close += 1; },
    async download(onEvent) {
      calls.download += 1;
      onEvent({ event: "Started", data: { contentLength: 10 } });
      onEvent({ event: "Progress", data: { chunkLength: 10 } });
      onEvent({ event: "Finished", data: {} });
    },
    async install() { calls.install += 1; },
  };
}

test("browser preview reports the build version without accessing the desktop update source", async () => {
  let checks = 0;
  const manager = createSoftwareUpdateManager({
    desktop: false,
    buildVersion: "0.2.2-preview",
    check: async () => { checks += 1; return null; },
  });
  await manager.initialize();
  assert.equal(manager.getSnapshot().currentVersion, "0.2.2-preview");
  assert.equal(manager.getSnapshot().phase, "idle");
  assert.match(manager.getSnapshot().message, /不访问桌面更新源/);
  assert.equal(checks, 0);
});

test("desktop initialization shares the real current and available versions without auto-downloading", async () => {
  const update = fakeUpdate("0.2.3");
  let checks = 0;
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "fallback",
    getCurrentVersion: async () => "0.2.2",
    check: async () => { checks += 1; return update; },
  });
  await manager.initialize();
  assert.equal(checks, 1);
  assert.deepEqual(manager.getSnapshot(), {
    desktop: true,
    currentVersion: "0.2.2",
    availableVersion: "0.2.3",
    phase: "available",
    message: "发现新版本 0.2.3，下载前不会启动安装。",
    progress: null,
    componentStatus: null,
    recovery: null,
  });
  assert.equal(update.calls.download, 0);
  assert.equal(update.calls.install, 0);
});

test("checks are deduplicated, throttled for six hours and close replaced update resources", async () => {
  let clock = 100;
  let checks = 0;
  const first = fakeUpdate("0.2.3");
  const second = fakeUpdate("0.2.4");
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "0.2.2",
    now: () => clock,
    getCurrentVersion: async () => "0.2.2",
    check: async () => {
      checks += 1;
      if (checks === 1) return first;
      await gate;
      return second;
    },
  });
  await manager.initialize();
  await manager.checkIfStale();
  assert.equal(checks, 1);
  clock += UPDATE_CHECK_INTERVAL_MS;
  const one = manager.checkIfStale();
  const two = manager.checkForUpdate();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(checks, 2);
  release();
  await Promise.all([one, two]);
  assert.equal(first.calls.close, 1);
  assert.equal(manager.getSnapshot().availableVersion, "0.2.4");
  await manager.dispose();
  assert.equal(second.calls.close, 1);
});

test("offline checks remain an error and never claim the app is current", async () => {
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "0.2.2",
    getCurrentVersion: async () => "0.2.2",
    check: async () => { throw new Error("connection reset"); },
  });
  await manager.initialize();
  assert.equal(manager.getSnapshot().phase, "error");
  assert.equal(manager.getSnapshot().availableVersion, null);
  assert.match(manager.getSnapshot().message, /检查更新失败/);
});

test("download and installation begin only after the explicit action", async () => {
  const update = fakeUpdate("0.2.3");
  let relaunches = 0;
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "0.2.2",
    getCurrentVersion: async () => "0.2.2",
    check: async () => update,
    relaunch: async () => { relaunches += 1; },
  });
  await manager.initialize();
  assert.equal(update.calls.download, 0);
  assert.equal(await manager.downloadAndInstall(), true);
  assert.equal(update.calls.download, 1);
  assert.equal(update.calls.install, 1);
  assert.equal(relaunches, 1);
});

test("disposing during a check closes the late update and publishes nothing else", async () => {
  const update = fakeUpdate("0.2.3");
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "0.2.2",
    check: async () => { await gate; return update; },
  });
  const pending = manager.checkForUpdate();
  await manager.dispose();
  release();
  await pending;
  assert.equal(update.calls.close, 1);
});
