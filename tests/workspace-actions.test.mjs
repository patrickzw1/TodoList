import assert from "node:assert/strict";
import { test } from "node:test";
import {
  archiveTasksById, deleteArchivedTasksById, deleteProjectWithTasks, moveProjectTasks,
  moveProjectTasksToNewProject, reconcileSelectedTaskIds, reorderTaskSubset,
} from "../src/workspace-actions.ts";

function task(id, projectId, archived = false) {
  return {
    id, projectId, title: id, description: "", status: "todo", priority: "medium", dueLabel: "未安排",
    dueDate: "9999-12-31", tags: [], source: "手动创建", archived, pinned: true, version: 1,
    subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [],
  };
}

function workspace() {
  return {
    version: 4,
    projects: [{ id: "source", name: "来源", color: "#111" }, { id: "target", name: "承接", color: "#222" }],
    tasks: [task("active", "source"), task("archived", "source", true), task("other", "target")],
  };
}

test("project deletion moves both active and archived tasks to an existing target in one update", () => {
  const result = moveProjectTasks(workspace(), "source", "target");
  assert.deepEqual(result.projects.map((project) => project.id), ["target"]);
  assert.deepEqual(result.tasks.map((item) => item.projectId), ["target", "target", "target"]);
  assert.equal(result.tasks[1].archived, true);
});

test("project deletion can create an explicit destination or permanently remove all related tasks", () => {
  const destination = { id: "new", name: "新承接", color: "#333" };
  const moved = moveProjectTasksToNewProject(workspace(), "source", destination);
  assert.deepEqual(moved.projects.map((project) => project.id), ["target", "new"]);
  assert.equal(moved.tasks.find((item) => item.id === "active").projectId, "new");
  const deleted = deleteProjectWithTasks(workspace(), "source");
  assert.deepEqual(deleted.tasks.map((item) => item.id), ["other"]);
  assert.deepEqual(deleted.projects.map((project) => project.id), ["target"]);
});

test("batch actions replay stable ids against latest archive state and leave hidden tasks alone", () => {
  const current = workspace();
  const archived = archiveTasksById(current, ["active"]);
  assert.equal(archived.tasks.find((item) => item.id === "active").archived, true);
  assert.equal(archived.tasks.find((item) => item.id === "active").pinned, false);
  assert.equal(archived.tasks.find((item) => item.id === "other").archived, false);

  const latest = workspace();
  latest.tasks.find((item) => item.id === "archived").archived = false;
  const deleted = deleteArchivedTasksById(latest, ["archived", "other"]);
  assert.deepEqual(deleted.tasks.map((item) => item.id), ["active", "archived", "other"]);
});

test("selection is limited to the current filtered result", () => {
  assert.deepEqual([...reconcileSelectedTaskIds(["a", "b", "hidden"], ["b", "c"])], ["b"]);
});

test("reordering a visible project subset preserves hidden and unrelated task slots", () => {
  const current = {
    version: 20,
    projects: workspace().projects,
    tasks: [
      task("visible-a", "source"),
      task("hidden", "source"),
      task("other", "target"),
      task("visible-b", "source"),
      task("archived", "source", true),
      task("visible-c", "source"),
    ],
  };
  current.tasks[1].description = "does not match the search";
  const reordered = reorderTaskSubset(
    current,
    "source",
    false,
    ["visible-a", "visible-b", "visible-c"],
    ["visible-c", "visible-a", "visible-b"],
  );
  assert.equal(reordered.version, 21);
  assert.deepEqual(reordered.tasks.map((item) => item.id), [
    "visible-c", "hidden", "other", "visible-a", "archived", "visible-b",
  ]);
  assert.deepEqual(reordered.tasks.find((item) => item.id === "other"), current.tasks[2]);
  assert.deepEqual(reordered.tasks.find((item) => item.id === "archived"), current.tasks[4]);
});

test("reordering rejects stale scope and skips no-op revisions", () => {
  const current = workspace();
  assert.equal(reorderTaskSubset(current, "source", false, ["active"], ["active"]), current);
  assert.throws(() => reorderTaskSubset(current, "source", false, ["active", "active"], ["active", "active"]), /重复/);
  assert.throws(() => reorderTaskSubset(current, "source", false, ["active", "archived"], ["archived", "active"]), /列表已变化/);
  assert.throws(() => reorderTaskSubset(current, "source", false, ["active", "other"], ["other", "active"]), /列表已变化/);
  assert.equal(current.version, 4);
});
