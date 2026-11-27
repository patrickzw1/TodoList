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

function checkResult(update, now = Date.now(), failure = null) {
  return { update, checked: true, status: {
    lastAttemptAt: now, lastCheckedAt: now,
    nextAutomaticCheckAt: now + UPDATE_CHECK_INTERVAL_MS,
    nextManualCheckAt: now + 60_000, retryAt: null,
    lastResult: failure ? "error" : update ? "available" : "latest", message: failure,
  } };
}

test("browser preview reports the build version without accessing the desktop update source", async () => {
  let checks = 0;
  const manager = createSoftwareUpdateManager({
    desktop: false,
    buildVersion: "0.2.2-preview",
    check: async () => { checks += 1; return checkResult(null); },
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
    now: () => 1_000,
    getCurrentVersion: async () => "0.2.2",
    check: async () => { checks += 1; return checkResult(update, 1_000); },
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
    lastAttemptAt: 1_000,
    lastCheckedAt: 1_000,
    nextAutomaticCheckAt: 1_000 + UPDATE_CHECK_INTERVAL_MS,
    nextManualCheckAt: 61_000,
    retryAt: null,
    checkBlockedUntil: 61_000,
    lastCheckResult: "available",
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
      if (checks === 1) return checkResult(first, clock);
      await gate;
      return checkResult(second, clock);
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
    check: async () => checkResult(update),
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
    check: async () => { await gate; return checkResult(update); },
  });
  const pending = manager.checkForUpdate();
  await manager.dispose();
  release();
  await pending;
  assert.equal(update.calls.close, 1);
});

test("highest release resource failures remain errors and six-hour throttling still applies", async () => {
  let clock = 100;
  let checks = 0;
  const message = "版本检查失败：最高正式版本 0.2.10 缺少 latest.json 更新清单。";
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "0.2.9",
    now: () => clock,
    check: async () => { checks += 1; return checkResult(null, clock, message); },
  });
  await manager.initialize();
  assert.equal(manager.getSnapshot().phase, "error");
  assert.equal(manager.getSnapshot().message, message);
  assert.equal(manager.getSnapshot().availableVersion, null);
  await manager.checkIfStale();
  assert.equal(checks, 1);
  clock += UPDATE_CHECK_INTERVAL_MS;
  await manager.checkIfStale();
  assert.equal(checks, 2);
});

test("Tauri download signature rejection never invokes installation or relaunch", async () => {
  const update = fakeUpdate("0.2.10");
  update.download = async () => { throw new Error("signature verification failed"); };
  let relaunches = 0;
  const manager = createSoftwareUpdateManager({
    desktop: true,
    buildVersion: "0.2.9",
    check: async () => checkResult(update),
    relaunch: async () => { relaunches += 1; },
  });
  await manager.initialize();
  assert.equal(await manager.downloadAndInstall(), false);
  assert.equal(update.calls.install, 0);
  assert.equal(relaunches, 0);
  assert.equal(manager.getSnapshot().phase, "error");
  assert.match(manager.getSnapshot().message, /签名验证失败/);
});

test("server cooldown blocks manual and focus checks without claiming latest", async () => {
  let clock = 1_000;
  let checks = 0;
  const manager = createSoftwareUpdateManager({
    desktop: true, buildVersion: "0.2.23", now: () => clock,
    check: async (automatic) => {
      checks += 1;
      assert.equal(automatic, true);
      return { update: null, checked: true, status: {
        lastAttemptAt: clock, lastCheckedAt: clock,
        nextAutomaticCheckAt: clock + UPDATE_CHECK_INTERVAL_MS,
        nextManualCheckAt: clock + 60_000, retryAt: clock + 120_000,
        lastResult: "error", message: "版本检查失败：GitHub 请求已限流。",
      } };
    },
  });
  await manager.initialize();
  await manager.checkForUpdate();
  await manager.checkIfStale();
  assert.equal(checks, 1);
  assert.equal(manager.getSnapshot().phase, "error");
  assert.equal(manager.getSnapshot().retryAt, 121_000);
  assert.equal(manager.getSnapshot().checkBlockedUntil, 121_000);
  assert.equal(manager.getSnapshot().availableVersion, null);
});

test("manual checking has a short interval and automatic focus still uses six hours", async () => {
  let clock = 1_000;
  const modes = [];
  const manager = createSoftwareUpdateManager({
    desktop: true, buildVersion: "0.2.23", now: () => clock,
    check: async (automatic) => { modes.push(automatic); return checkResult(null, clock); },
  });
  await manager.initialize();
  await manager.checkForUpdate();
  clock = 60_999;
  await manager.checkForUpdate();
  assert.deepEqual(modes, [true]);
  clock = 61_000;
  await manager.checkForUpdate();
  await manager.checkIfStale();
  assert.deepEqual(modes, [true, false]);
  clock += UPDATE_CHECK_INTERVAL_MS;
  await manager.checkIfStale();
  assert.deepEqual(modes, [true, false, true]);
});

test("restart receives remembered status without presenting it as a fresh successful check", async () => {
  for (const previous of ["latest", "available", "error"]) {
    const status = checkResult(null, 1_000).status;
    status.lastResult = previous;
    status.message = previous === "error" ? "版本检查失败：GitHub Release 服务返回 HTTP 403 Forbidden，请稍后重试。" : null;
    const manager = createSoftwareUpdateManager({
      desktop: true, buildVersion: "0.2.23", now: () => 2_000,
      check: async () => ({ update: null, checked: false, status }),
    });
    await manager.initialize();
    assert.equal(manager.getSnapshot().phase, previous === "error" ? "error" : "idle");
    assert.equal(manager.getSnapshot().availableVersion, null);
    assert.equal(manager.getSnapshot().lastCheckedAt, 1_000);
    assert.equal(manager.getSnapshot().retryAt, null);
    assert.doesNotMatch(manager.getSnapshot().message, /当前已经是最新/);
  }
});

test("a backend skip after another window checked closes the old install handle", async () => {
  let clock = 1_000;
  let calls = 0;
  const update = fakeUpdate("0.2.24");
  const manager = createSoftwareUpdateManager({
    desktop: true, buildVersion: "0.2.23", now: () => clock,
    check: async () => {
      calls += 1;
      const result = checkResult(calls === 1 ? update : null, clock);
      result.checked = calls === 1;
      return result;
    },
  });
  await manager.initialize();
  clock = 61_000;
  await manager.checkForUpdate();
  assert.equal(update.calls.close, 1);
  assert.equal(manager.getSnapshot().availableVersion, null);
  assert.equal(manager.getSnapshot().phase, "idle");
  assert.equal(await manager.downloadAndInstall(), false);
  assert.equal(update.calls.install, 0);
});
