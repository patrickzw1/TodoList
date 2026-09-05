import assert from "node:assert/strict";
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
