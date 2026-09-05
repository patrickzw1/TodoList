import assert from "node:assert/strict";
import { test } from "node:test";
import { BACKUP_SCHEMA_VERSION, createWorkspaceBackup, importedWorkspaceForSave, parseWorkspaceBackup } from "../src/workspace-backup.ts";

function workspace() {
  return {
    version: 8,
    projects: [{ id: "project-1", name: "TodoList", color: "#1264f4" }],
    tasks: [{
      id: "task-1", projectId: "project-1", title: "Backup", description: "", status: "todo", priority: "medium",
      dueLabel: "未安排", dueDate: "9999-12-31", tags: [], source: "手动创建", archived: false, pinned: false,
      version: 2, subtasks: [], acceptanceCriteria: [], dependencies: [], activity: [],
    }],
  };
}

test("TodoList backup round trips its metadata and workspace", () => {
  const backup = createWorkspaceBackup(workspace(), 1788494400, "0.1.0");
  const parsed = parseWorkspaceBackup(JSON.stringify(backup));
  assert.equal(parsed.schemaVersion, BACKUP_SCHEMA_VERSION);
  assert.equal(parsed.exportedAt, 1788494400);
  assert.equal(parsed.workspace.projects[0].name, "TodoList");
  assert.equal(parsed.workspace.tasks[0].title, "Backup");
});

test("TodoList backup rejects unsupported versions and malformed JSON", () => {
  const backup = createWorkspaceBackup(workspace());
  assert.throws(() => parseWorkspaceBackup(JSON.stringify({ ...backup, schemaVersion: BACKUP_SCHEMA_VERSION + 1 })), /不支持备份格式版本/);
  assert.throws(() => parseWorkspaceBackup("not json"), /不是有效的 TodoList JSON/);
});

test("TodoList backup rejects duplicate ids and orphaned tasks", () => {
  const duplicate = workspace();
  duplicate.projects.push({ ...duplicate.projects[0] });
  assert.throws(() => createWorkspaceBackup(duplicate), /重复的项目 ID/);

  const orphan = workspace();
  orphan.tasks[0].projectId = "missing";
  assert.throws(() => createWorkspaceBackup(orphan), /不存在的项目/);
});

test("import uses the latest persisted version instead of the backup version", () => {
  const imported = workspace();
  imported.version = 2;
  const current = { ...workspace(), version: 41 };
  const restored = importedWorkspaceForSave(imported, current);
  assert.equal(restored.version, 42);
  assert.ok(restored.tasks[0].version > current.tasks[0].version);
  assert.ok(importedWorkspaceForSave(imported, restored).tasks[0].version > restored.tasks[0].version);
  assert.equal(imported.version, 2);
});

test("restore rebases every task beyond current and imported versions without mutating inputs", () => {
  const current = workspace();
  current.tasks[0].version = 80;
  const imported = workspace();
  imported.tasks.push({ ...imported.tasks[0], id: "old-deleted-task", version: 200 });
  const restored = importedWorkspaceForSave(imported, current);
  assert.deepEqual(restored.tasks.map((task) => task.version), [201, 201]);
  assert.equal(current.tasks[0].version, 80);
  assert.equal(imported.tasks[0].version, 2);
  assert.throws(() => importedWorkspaceForSave(imported, { ...current, version: Number.MAX_SAFE_INTEGER }), /安全上限/);
});
