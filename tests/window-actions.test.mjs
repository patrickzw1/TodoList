import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { afterEach, beforeEach, test } from "node:test";
import { mockIPC } from "@tauri-apps/api/mocks";
import { openMainWindow, openStickyWindow } from "../src/window-actions.ts";

beforeEach(() => { globalThis.window = {}; });
afterEach(() => { delete globalThis.window; });

test("desktop opens the main window through the native lifecycle command", async () => {
  const commands = [];
  mockIPC((command) => { commands.push(command); });
  await openMainWindow();
  assert.deepEqual(commands, ["open_main_window"]);
});

test("desktop exposes an opening failure so the UI can report it", async () => {
  mockIPC(() => { throw new Error("window creation failed"); });
  await assert.rejects(openMainWindow, /window creation failed/);
});

test("web preview focuses its existing opener", async () => {
  let focused = 0;
  window.opener = { closed: false, focus: () => { focused += 1; } };
  window.open = () => { assert.fail("must not duplicate a live task board"); };
  await openMainWindow();
  assert.equal(focused, 1);
});

test("web preview reopens the task board when its opener was closed or absent", async () => {
  for (const opener of [null, { closed: true }]) {
    const calls = [];
    let focused = 0;
    window.opener = opener;
    window.open = (...args) => {
      calls.push(args);
      return { focus: () => { focused += 1; } };
    };
    await openMainWindow();
    assert.deepEqual(calls, [["/", "todolist-main"]]);
    assert.equal(focused, 1);
  }
});

test("web preview reports popup blocking instead of silently succeeding", async () => {
  window.open = () => null;
  await assert.rejects(openMainWindow, /浏览器阻止/);
});

test("only explicit sticky mode asks the native command to minimize the main window", async () => {
  const calls = [];
  mockIPC((command, args) => {
    calls.push({ command, args });
  });
  await openStickyWindow();
  await openStickyWindow(true);
  assert.deepEqual(calls, [
    { command: "open_sticky_window", args: { minimizeMain: false } },
    { command: "open_sticky_window", args: { minimizeMain: true } },
  ]);
});

test("sticky opening failure propagates instead of continuing as successful", async () => {
  mockIPC(() => { throw new Error("sticky creation failed"); });
  await assert.rejects(() => openStickyWindow(true), /sticky creation failed/);
});

test("web sticky mode opens a named note without minimizing or closing the browser", async () => {
  const calls = [];
  window.close = () => { assert.fail("web preview must remain open"); };
  window.open = (...args) => { calls.push(args); return {}; };
  await openStickyWindow(true);
  assert.deepEqual(calls, [["/?mode=sticky", "todolist-sticky", "popup,width=370,height=470"]]);
});

test("web sticky popup blocking is reported to the main UI", async () => {
  window.open = () => null;
  await assert.rejects(openStickyWindow, /浏览器阻止/);
});

test("sticky layout fills a resized viewport without changing its native window contract", async () => {
  const [styles, nativeWindowActions] = await Promise.all([
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/window_actions.rs", import.meta.url), "utf8"),
  ]);
  assert.match(styles, /html:has\(\.sticky-window\)[^{}]*body:has\(\.sticky-window\) #root[^{}]*\{[^}]*width: 100%;[^}]*height: 100%/);
  assert.match(styles, /\.sticky-window \{[^}]*width: 100%;[^}]*height: 100%;[^}]*min-width: 0;[^}]*min-height: 0;/);
  assert.doesNotMatch(styles, /\.sticky-window \{[^}]*width: min\(100vw, 360px\)/);
  assert.match(styles, /\.sticky-body \{[^}]*flex: 1;[^}]*min-height: 0;[^}]*overflow: auto;/);
  assert.match(nativeWindowActions, /\.min_inner_size\(320\.0, 220\.0\)/);
  assert.match(nativeWindowActions, /\.resizable\(true\)/);
  assert.match(nativeWindowActions, /\.transparent\(false\)/);
  assert.doesNotMatch(nativeWindowActions, /\.transparent\(true\)/);
  assert.match(nativeWindowActions, /\.background_color\(tauri::window::Color\(255, 254, 248, 255\)\)/);
  assert.match(styles, /body:has\(\.sticky-window\) \{[^}]*background: #fffef8;/);
});

test("task detail leaves wheel delivery on the exposed workspace while blocking outside pointer actions", async () => {
  const app = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(app, /detail-backdrop/);
  assert.match(app, /onPointerDownCapture=\{beginOutsideDetailInteraction\}/);
  assert.match(app, /onPointerUpCapture=\{endOutsideDetailInteraction\}/);
  assert.match(app, /onPointerCancelCapture=\{\(event\) => endOutsideDetailInteraction\(event, true\)\}/);
  assert.match(app, /onClickCapture=\{handleOutsideDetailClick\}/);
  assert.match(app, /target\.closest\("\.detail-panel, \.image-viewer, \.modal-backdrop"\)/);
  assert.match(app, /if \(event\.pointerType === "mouse"\) event\.preventDefault\(\)/);
  assert.doesNotMatch(app, /onWheel|WheelEvent|dispatchEvent\([^)]*wheel/i);
});

test("task detail keeps header and footer outside its independent scroll region", async () => {
  const [app, styles] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
  ]);
  const detail = app.slice(app.indexOf("function TaskDetail"), app.indexOf("function ScrollableSettingsPage"));
  assert.ok(detail.indexOf('className="detail-header"') < detail.indexOf('className="detail-scroll auto-hide-scrollbar"'));
  assert.ok(detail.indexOf('className="detail-scroll auto-hide-scrollbar"') < detail.indexOf('className="detail-actions"'));
  assert.match(styles, /\.detail-panel \{[^}]*overflow: hidden;/);
  assert.match(styles, /\.detail-header \{[^}]*flex: 0 0 auto;/);
  assert.match(styles, /\.detail-scroll \{[^}]*flex: 1 1 auto;[^}]*min-height: 0;[^}]*overflow: auto;/);
  assert.match(styles, /\.detail-actions \{[^}]*flex: 0 0 auto;/);
});

test("real overflow regions and editor textareas use the shared scrollbar treatment", async () => {
  const [app, taskFiles, dialogs, styles] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/TaskFiles.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/dialogs.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
  ]);
  assert.match(app, /workspace-content auto-hide-scrollbar/);
  assert.match(app, /sticky-body auto-hide-scrollbar/);
  assert.match(taskFiles, /image-viewer-canvas auto-hide-scrollbar/);
  assert.match(dialogs, /function AutoHideTextarea[\s\S]*useAutoHideScrollbar<HTMLTextAreaElement>/);
  assert.match(dialogs, /project-dialog auto-hide-scrollbar/);
  assert.match(dialogs, /project-delete-dialog auto-hide-scrollbar/);
  assert.match(dialogs, /confirm-dialog auto-hide-scrollbar/);
  assert.match(styles, /\.create-dialog \{[^}]*max-height:[^;]+;[^}]*overflow: auto;/);
  assert.match(styles, /\.project-nav \{[^}]*overflow-y: auto;[^}]*scrollbar-width: none;/);
  assert.match(styles, /\.project-nav::-webkit-scrollbar \{[^}]*display: none;/);
});

test("integration and settings share the auto-hiding scroll container", async () => {
  const [app, styles] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
  ]);
  assert.match(app, /function ScrollableSettingsPage[\s\S]*useAutoHideScrollbar<HTMLDivElement>\(\)[\s\S]*settings-page auto-hide-scrollbar/);
  assert.match(app, /<ScrollableSettingsPage label="连接与权限设置">/);
  assert.match(app, /<ScrollableSettingsPage label="TodoList 设置">/);
  assert.match(styles, /\.settings-page \{[^}]*height: 100%;[^}]*overflow-y: auto;/);
  assert.match(styles, /\.auto-hide-scrollbar\.is-scroll-active[\s\S]*\.auto-hide-scrollbar\.is-scrollbar-near/);
});
