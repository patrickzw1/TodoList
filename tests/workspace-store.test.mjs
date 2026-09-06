import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { boundedActivity, taskWithUserActivity } from "../src/task-activity.ts";
import { validateWorkspaceCompletion } from "../src/task-validation.ts";
import { importedWorkspaceForSave } from "../src/workspace-backup.ts";
import { checklistEditsOnLatest } from "../src/task-checklists.ts";

function fixture(version = 10) {
  return { version, projects: [{ id: "p", name: "P", color: "#fff" }], tasks: [{
    id: "t", projectId: "p", title: "Task", description: "", status: "todo", priority: "medium",
    dueLabel: "", dueDate: "9999-12-31", tags: [], source: "手动创建", archived: false, pinned: true,
    version: 1, subtasks: [], acceptanceCriteria: [], dependencies: [], activity: [],
  }] };
}

// Run the real hook with in-memory React hooks, storage and IPC. No user database,
// timers or browser profile are involved, and all queued operations are flushed.
function driver({ desktop = true, cacheFails = false, readFails = false, beforeSave, initialDatabase, initialCache, search = "" } = {}) {
  let database = initialDatabase === undefined ? fixture(11) : initialDatabase;
  let cached = initialCache === undefined ? fixture() : initialCache;
  let slot = 0, first = true;
  const states = [], effects = [], listeners = new Map(), timers = [], calls = [];
  const react = {
    useState(initial) {
      const index = slot++;
      if (first) states[index] = typeof initial === "function" ? initial() : initial;
      return [states[index], (value) => { states[index] = typeof value === "function" ? value(states[index]) : value; }];
    },
    useRef(initial) { const index = slot++; if (first) states[index] = { current: initial }; return states[index]; },
    useCallback(callback) { return callback; },
    useEffect(effect) { if (first) effects.push(effect); },
  };
  const window = {
    ...(desktop ? { __TAURI_INTERNALS__: {} } : {}),
    localStorage: {
      getItem() { if (readFails) throw new Error("SecurityError"); return cached === null ? null : JSON.stringify(cached); },
      setItem(key, value) { if (cacheFails) throw new Error("QuotaExceededError"); cached = JSON.parse(value); },
    },
    addEventListener(name, callback) { listeners.set(name, callback); },
    removeEventListener() {},
    dispatchEvent(event) { listeners.get(event.type)?.(); },
    setInterval(callback) { timers.push(callback); return timers.length; },
    clearInterval() {},
    location: { search },
  };
  const invoke = async (command, args) => {
    calls.push(command);
    if (command === "load_workspace") return structuredClone(database);
    if (command === "load_workspace_version") return database?.version ?? null;
    if (beforeSave) { const hook = beforeSave; beforeSave = undefined; hook(database); }
    if (database && args.workspace.version <= database.version) throw new Error("workspace version conflict");
    database = structuredClone(args.workspace);
    if (command === "restore_workspace") {
      for (const task of database.tasks) task.version = 500;
      return structuredClone(database);
    }
  };
  const dependencies = {
    react, "@tauri-apps/api/core": { invoke }, "./seed": { seedWorkspace: fixture() },
    "./task-activity": { boundedActivity }, "./task-validation": { validateWorkspaceCompletion },
  };
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL("../src/workspace-store.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, { exports, require: (name) => {
    assert.ok(dependencies[name], `Unexpected import ${name}`); return dependencies[name];
  }, window, structuredClone, console, URLSearchParams, CustomEvent: class { constructor(type) { this.type = type; } } });
  const render = () => { slot = 0; const hook = exports.useWorkspace(true); first = false; return hook; };
  render();
  return {
    render, calls, get database() { return database; }, get cached() { return cached; },
    async flush() { for (let i = 0; i < 6; i++) await new Promise((resolve) => setImmediate(resolve)); return render(); },
    async mount() { for (const effect of effects) effect(); return this.flush(); },
    async poll() { for (const timer of timers) timer(); return this.flush(); },
    failCache(value) { cacheFails = value; },
  };
}

test("new desktop and browser workspaces start empty while the browser demo remains explicit", async () => {
  const desktop = driver({ initialDatabase: null, initialCache: null });
  const desktopHook = await desktop.mount();
  assert.deepEqual(desktopHook.workspace.projects, []);
  assert.deepEqual(desktopHook.workspace.tasks, []);
  assert.deepEqual(desktop.database.tasks, []);

  const browser = driver({ desktop: false, initialCache: null });
  assert.deepEqual((await browser.mount()).workspace.tasks, []);
  const demo = driver({ desktop: false, initialCache: null, search: "?demo=1" });
  assert.equal((await demo.mount()).workspace.tasks[0].title, "Task");
});

const update = (patch) => (current) => ({ ...current, version: current.version + 1,
  tasks: current.tasks.map((task) => taskWithUserActivity(task, "用户操作", patch)),
});

test("desktop cache quota cannot hide a loaded workspace or turn a saved database write into failure", async () => {
  const run = driver({ cacheFails: true });
  let hook = await run.mount();
  assert.equal(hook.workspace.version, 11);
  assert.equal(hook.storageState, "saved");
  hook.commit(update({ title: "Saved" }));
  hook = await run.flush();
  assert.equal(run.database.tasks[0].title, "Saved");
  assert.equal(hook.workspace.version, run.database.version);
  assert.equal(hook.storageState, "saved");
  run.database.version++;
  run.database.tasks[0].title = "MCP";
  hook = await run.poll();
  assert.equal(hook.workspace.tasks[0].title, "MCP");
  const reads = run.calls.filter((command) => command === "load_workspace").length;
  await run.poll();
  assert.equal(run.calls.filter((command) => command === "load_workspace").length, reads);
});

test("desktop still loads SQLite when reading localStorage is prohibited", async () => {
  const run = driver({ readFails: true, cacheFails: true });
  assert.equal((await run.mount()).workspace.version, 11);
});

test("browser persistence failure is visible, rolls back and permits the next save", async () => {
  const run = driver({ desktop: false });
  let hook = await run.mount();
  run.failCache(true);
  hook.commit(update({ title: "Unsaved" }));
  hook = await run.flush();
  assert.equal(hook.storageState, "error");
  assert.match(hook.storageMessage, /QuotaExceededError/);
  assert.equal(hook.workspace.tasks[0].title, "Task");
  assert.equal(run.cached.tasks[0].title, "Task");
  run.failCache(false);
  hook.commit(update({ title: "Retry" }));
  hook = await run.flush();
  assert.equal(hook.storageState, "saved");
  assert.equal(run.cached.tasks[0].title, "Retry");
});

for (const action of ["完成任务", "状态更新为已完成", "从便签完成任务"]) {
  test(`${action} rejects completion when MCP adds an unconfirmed criterion before save`, async () => {
    const run = driver({ beforeSave(database) {
      database.version++;
      database.tasks[0].version++;
      database.tasks[0].acceptanceCriteria.push({ id: "new", title: "MCP criterion", completed: false });
    } });
    let hook = await run.mount();
    hook.commit((current) => ({ ...current, version: current.version + 1,
      tasks: current.tasks.map((task) => taskWithUserActivity(task, action, { status: "done" })),
    }));
    hook = await run.flush();
    assert.equal(hook.storageState, "error");
    assert.match(hook.storageMessage, /验收标准/);
    assert.equal(run.database.tasks[0].status, "todo");
    assert.equal(hook.workspace.tasks[0].acceptanceCriteria.length, 1);
    assert.equal(run.calls.filter((command) => command === "save_workspace").length, 1);
  });
}

test("conflict replay still preserves unrelated MCP changes when acceptance is confirmed", async () => {
  const run = driver({ beforeSave(database) {
    database.version++;
    database.tasks[0].description = "MCP description";
    database.tasks[0].acceptanceCriteria.push({ id: "a", title: "Verified", completed: true });
  } });
  const hook = await run.mount();
  hook.commit(update({ status: "done" }));
  const saved = await run.flush();
  assert.equal(saved.storageState, "merged");
  assert.equal(saved.workspace.tasks[0].status, "done");
  assert.equal(saved.workspace.tasks[0].description, "MCP description");
});

test("an action queued behind a rejected completion preserves the latest MCP criteria", async () => {
  const run = driver({ beforeSave(database) {
    database.version++;
    database.tasks[0].version++;
    database.tasks[0].acceptanceCriteria.push({ id: "new", title: "MCP criterion", completed: false });
  } });
  const hook = await run.mount();
  hook.commit(update({ status: "done" }));
  hook.commit(update({ title: "Queued title" }));
  const saved = await run.flush();
  assert.equal(run.database.tasks[0].status, "todo");
  assert.equal(run.database.tasks[0].acceptanceCriteria[0].title, "MCP criterion");
  assert.equal(run.database.tasks[0].title, "Queued title");
  assert.equal(saved.workspace.version, run.database.version);
});

test("restore retries on a concurrent MCP write and queued UI actions use authoritative restored versions", async () => {
  const run = driver({ beforeSave(database) { database.version++; database.tasks[0].version = 90; } });
  const hook = await run.mount();
  hook.commit((current) => importedWorkspaceForSave(fixture(), current), true);
  hook.commit(update({ title: "After restore" }));
  const saved = await run.flush();
  assert.equal(run.calls.filter((command) => command === "restore_workspace").length, 2);
  assert.equal(saved.workspace.tasks[0].version, 501);
  assert.equal(run.database.tasks[0].title, "After restore");
});

test("an editor retry preserves latest checkbox state instead of re-confirming an MCP change", async () => {
  const run = driver({ beforeSave(database) {
    database.version++;
    database.tasks[0].acceptanceCriteria[0].completed = false;
  } });
  run.database.tasks[0].acceptanceCriteria = [
    { id: "a", title: "A", completed: true }, { id: "b", title: "B", completed: true },
  ];
  const hook = await run.mount();
  const edits = { status: "done", acceptanceCriteria: [...hook.workspace.tasks[0].acceptanceCriteria].reverse() };
  hook.commit((current) => ({ ...current, version: current.version + 1,
    tasks: current.tasks.map((task) => taskWithUserActivity(task, "编辑任务", checklistEditsOnLatest(task, edits))),
  }));
  const saved = await run.flush();
  assert.equal(saved.storageState, "error");
  assert.equal(saved.workspace.tasks[0].status, "todo");
  assert.equal(saved.workspace.tasks[0].acceptanceCriteria[0].completed, false);
});
