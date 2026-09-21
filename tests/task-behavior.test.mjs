import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { MAX_TASK_ACTIVITY_ITEMS, activityItemsForDetail, boundedActivity, formatActivityTime, taskWithUserActivity } from "../src/task-activity.ts";
import { resolveDefaultProjectId } from "../src/task-creation.ts";
import { searchTasks, tasksForView } from "../src/task-filtering.ts";
import { applyChecklistEditorEdit, checklistEditorEdit } from "../src/task-checklists.ts";
import { calendarMonthCells, isDueDateOnOrBefore, isValidIsoDate, normalizeOptionalDueDate, shiftIsoDate, shiftIsoMonth, UNSCHEDULED_DUE_DATE } from "../src/date-utils.ts";
import { isPresetProjectColor, normalizeProjectColor, PROJECT_COLORS, PROJECT_COLOR_PRESETS } from "../src/project-colors.ts";

function makeTask(activity = []) {
  return {
    id: "task-1", projectId: "project-1", title: "Task", description: "", status: "todo", priority: "medium",
    dueLabel: "今天", dueDate: "2026-09-04", tags: [], source: "手动创建", archived: false, pinned: false,
    version: 1, subtasks: [], acceptanceCriteria: [], dependencies: [], activity,
  };
}

test("project colors keep presets, include named bright red, and normalize supported custom HEX values", () => {
  assert.equal(PROJECT_COLORS.length, 7);
  assert.deepEqual(PROJECT_COLOR_PRESETS.at(-1), { value: "#ff0000", label: "大红色" });
  assert.equal(isPresetProjectColor("#ff0000"), true);
  assert.equal(normalizeProjectColor(" #AbC "), "#aabbcc");
  assert.equal(normalizeProjectColor("#12A4f0"), "#12a4f0");
  for (const invalid of ["abc", "#12", "#1234", "#12345g", "red", ""]) assert.equal(normalizeProjectColor(invalid), null);
  assert.equal(isPresetProjectColor(PROJECT_COLORS[0]), true);
  assert.equal(isPresetProjectColor("#aabbcc"), false);
});

test("checklist editor edits preserve stable IDs and replay only explicit item changes", () => {
  const original = [{ id: "a", title: "A", completed: true }, { id: "b", title: "B", completed: false }];
  const edit = checklistEditorEdit(original, [
    { id: "a", title: " A renamed\nwith detail " },
    { id: "new", title: "New\nitem" },
  ]);
  assert.deepEqual(edit, {
    deletedIds: ["b"],
    titleEdits: [{ id: "a", title: "A renamed\nwith detail" }],
    additions: [{ id: "new", title: "New\nitem", completed: false }],
  });

  const latest = [
    { id: "a", title: "A changed elsewhere", completed: false },
    { id: "b", title: "B", completed: true },
    { id: "concurrent", title: "Added concurrently", completed: true },
  ];
  assert.deepEqual(applyChecklistEditorEdit(latest, edit), [
    { id: "a", title: "A renamed\nwith detail", completed: false },
    { id: "concurrent", title: "Added concurrently", completed: true },
    { id: "new", title: "New\nitem", completed: false },
  ]);
});

test("checklist conflict replay does not revive concurrent deletions or overwrite untouched items", () => {
  const original = [{ id: "a", title: "A", completed: false }, { id: "b", title: "B", completed: false }];
  const edit = checklistEditorEdit(original, [
    { id: "a", title: "A renamed" },
    { id: "b", title: "B" },
    { id: "new", title: "New" },
  ]);
  const latest = [{ id: "b", title: "B changed concurrently", completed: true }];
  assert.deepEqual(applyChecklistEditorEdit(latest, edit), [
    { id: "b", title: "B changed concurrently", completed: true },
    { id: "new", title: "New", completed: false },
  ]);
});

test("checklist drafts ignore blank new items but reject blank persisted items", () => {
  const original = [{ id: "a", title: "A", completed: true }];
  assert.equal(checklistEditorEdit(original, [{ id: "a", title: "A" }, { id: "new", title: " \n " }]), undefined);
  assert.throws(() => checklistEditorEdit(original, [{ id: "a", title: " " }]), /empty/);
  assert.throws(() => checklistEditorEdit(original, [{ id: "a", title: "A" }, { id: "a", title: "again" }]), /duplicate/);
});

test("task editor uses item-by-item multiline sections and fixed centered delete controls", () => {
  const dialogs = readFileSync(new URL("../src/dialogs.tsx", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.equal((dialogs.match(/<TaskEntryEditor sectionId=/g) ?? []).length, 3);
  assert.match(dialogs, /window\.requestAnimationFrame\(\(\) => document\.getElementById\(taskEntryControlId\(sectionId, id\)\)\?\.focus\(\)\)/);
  assert.doesNotMatch(dialogs, /每行一个|每行一项/);
  assert.match(styles, /\.task-entry-row \{[^}]*grid-template-columns: 22px minmax\(0, 1fr\) 32px;[^}]*align-items: center;/);
  assert.match(styles, /\.task-entry-delete \{[^}]*align-self: center;[^}]*width: 32px;[^}]*height: 32px;/);
  assert.match(styles, /\.dependency > span \{[^}]*overflow-wrap: anywhere;[^}]*white-space: pre-wrap;/);
});

test("task actions enforce acceptance on latest data and allow unrelated legacy edits", () => {
  const pending = { ...makeTask(), acceptanceCriteria: [{ id: "a", title: "Verify", completed: false }] };
  assert.throws(() => taskWithUserActivity(pending, "完成任务", { status: "done" }), /验收标准/);
  const legacy = { ...pending, status: "done" };
  assert.equal(taskWithUserActivity(legacy, "编辑", { title: "New title" }).status, "done");
  assert.equal(taskWithUserActivity(legacy, "重开", { status: "todo" }).status, "todo");
  assert.throws(() => taskWithUserActivity(legacy, "编辑", { acceptanceCriteria: [...legacy.acceptanceCriteria, { id: "b", title: "New", completed: false }] }), /验收标准/);
});

test("new-task project selection is safe when no project exists", () => {
  assert.equal(resolveDefaultProjectId([]), null);
  assert.equal(resolveDefaultProjectId([{ id: "project-1", name: "One", color: "#fff" }], "missing"), "project-1");
  assert.equal(resolveDefaultProjectId([{ id: "project-1", name: "One", color: "#fff" }], "project-1"), "project-1");
});

test("activity keeps only the newest bounded entries", () => {
  const activity = Array.from({ length: MAX_TASK_ACTIVITY_ITEMS + 5 }, (_, index) => ({ id: String(index), action: "edit", actor: "user", at: "legacy" }));
  const bounded = boundedActivity(activity);
  assert.equal(bounded.length, MAX_TASK_ACTIVITY_ITEMS);
  assert.equal(bounded[0].id, "5");
  assert.equal(bounded.at(-1).id, String(MAX_TASK_ACTIVITY_ITEMS + 4));
});

test("activity detail shows the latest four by default and all entries on request", () => {
  const activity = Array.from({ length: 6 }, (_, index) => ({ id: String(index), action: "edit", actor: "user", at: "legacy" }));
  assert.deepEqual(activityItemsForDetail(activity, false).map((item) => item.id), ["5", "4", "3", "2"]);
  assert.deepEqual(activityItemsForDetail(activity, true).map((item) => item.id), ["5", "4", "3", "2", "1", "0"]);
});

test("user activity records a real timestamp and remains bounded", () => {
  globalThis.crypto ??= { randomUUID: () => "activity-new" };
  const now = new Date("2026-09-04T06:30:00.000Z");
  const updated = taskWithUserActivity(makeTask(Array.from({ length: MAX_TASK_ACTIVITY_ITEMS }, (_, index) => ({ id: String(index), action: "edit", actor: "user", at: "legacy" }))), "编辑任务", { title: "Updated" }, now);
  assert.equal(updated.title, "Updated");
  assert.equal(updated.version, 2);
  assert.equal(updated.activity.length, MAX_TASK_ACTIVITY_ITEMS);
  assert.equal(updated.activity.at(-1).at, now.toISOString());
  assert.notEqual(formatActivityTime(updated.activity.at(-1).at), "刚刚");
  assert.equal(formatActivityTime("刚刚"), "时间未记录");
});

test("all view includes unscheduled work but excludes archived tasks", () => {
  const unscheduled = { ...makeTask(), id: "unscheduled", dueDate: "9999-12-31" };
  const completed = { ...makeTask(), id: "completed", status: "done" };
  const archived = { ...makeTask(), id: "archived", archived: true };

  assert.deepEqual(
    tasksForView([unscheduled, completed, archived], { kind: "all" }, "2026-09-05").map((task) => task.id),
    ["unscheduled", "completed"],
  );
});

test("today includes only valid dates on or before today and leaves unscheduled legacy data in broader views", () => {
  const noDateTasks = [
    { ...makeTask(), id: "empty", dueLabel: "未安排", dueDate: "" },
    { ...makeTask(), id: "whitespace", dueLabel: "未安排", dueDate: "   " },
    { ...makeTask(), id: "sentinel", dueLabel: "未安排", dueDate: UNSCHEDULED_DUE_DATE },
    { ...makeTask(), id: "invalid-format", dueLabel: "未安排", dueDate: "2026-9-8" },
    { ...makeTask(), id: "invalid-calendar", dueLabel: "未安排", dueDate: "2026-02-29" },
  ];
  const datedTasks = [
    { ...makeTask(), id: "overdue", status: "done", priority: "low", dueDate: "2026-09-07" },
    { ...makeTask(), id: "today", status: "blocked", priority: "high", dueDate: "2026-09-08" },
    { ...makeTask(), id: "future", dueDate: "2026-09-09" },
    { ...makeTask(), id: "archived-overdue", dueDate: "2026-09-07", archived: true },
  ];
  const tasks = [...noDateTasks, ...datedTasks];

  assert.deepEqual(
    tasksForView(tasks, { kind: "today" }, "2026-09-08").map((task) => task.id),
    ["overdue", "today"],
  );
  assert.deepEqual(
    tasksForView(tasks, { kind: "all" }, "2026-09-08").map((task) => task.id),
    [...noDateTasks.map((task) => task.id), "overdue", "today", "future"],
  );
  assert.deepEqual(
    tasksForView(tasks, { kind: "project", projectId: "project-1" }, "2026-09-08").map((task) => task.id),
    [...noDateTasks.map((task) => task.id), "overdue", "today", "future"],
  );
});

test("date helpers validate calendar dates and UI clearing uses the unscheduled sentinel", () => {
  assert.equal(isDueDateOnOrBefore("2024-02-29", "2024-02-29"), true);
  assert.equal(isDueDateOnOrBefore("2026-02-29", "2026-09-08"), false);
  assert.equal(isDueDateOnOrBefore("", "2026-09-08"), false);
  assert.equal(isDueDateOnOrBefore(UNSCHEDULED_DUE_DATE, "2026-09-08"), false);
  assert.equal(normalizeOptionalDueDate(""), UNSCHEDULED_DUE_DATE);
  assert.equal(normalizeOptionalDueDate("   "), UNSCHEDULED_DUE_DATE);
  assert.equal(normalizeOptionalDueDate("2026-09-08"), "2026-09-08");
  assert.equal(isValidIsoDate("2024-02-29"), true);
  assert.equal(isValidIsoDate("2026-02-29"), false);
});

test("calendar helpers produce a stable six-week grid and keyboard-safe date movement", () => {
  const september = calendarMonthCells(2026, 9);
  assert.equal(september.length, 42);
  assert.deepEqual(september[0], { date: "2026-08-30", day: 30, inCurrentMonth: false });
  assert.deepEqual(september.at(-1), { date: "2026-10-10", day: 10, inCurrentMonth: false });
  assert.equal(shiftIsoDate("2026-09-01", -1), "2026-08-31");
  assert.equal(shiftIsoDate("2024-02-28", 1), "2024-02-29");
  assert.equal(shiftIsoMonth("2026-01-31", 1), "2026-02-28");
  assert.equal(shiftIsoMonth("2024-01-31", 1), "2024-02-29");
  assert.equal(calendarMonthCells(1, 1)[0].date, null);
  assert.equal(calendarMonthCells(9999, 12).at(-1).date, null);
});

test("task detail tags wrap inside their metadata column and the editor uses a controllable date picker", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  const dialogs = readFileSync(new URL("../src/dialogs.tsx", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.match(app, /className="task-meta-tags"/);
  assert.match(styles, /\.task-meta > div \{[^}]*grid-template-columns: 84px minmax\(0, 1fr\)/);
  assert.match(styles, /\.tags \{[^}]*max-width: 100%;[^}]*flex-wrap: wrap;/);
  assert.match(styles, /\.tags span \{[^}]*max-width: 100%;[^}]*overflow-wrap: anywhere;[^}]*white-space: normal;/);
  assert.doesNotMatch(dialogs, /type="date"/);
  assert.match(dialogs, /role="dialog"[\s\S]*aria-label="选择截止日期"/);
  assert.match(dialogs, /aria-label="年份"[\s\S]*aria-label="月份"/);
  assert.match(dialogs, /清除[\s\S]*今天/);
  assert.match(dialogs, /ArrowLeft[\s\S]*ArrowRight[\s\S]*ArrowUp[\s\S]*ArrowDown/);
});

test("list and board render the same filtered visible task collection", () => {
  const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(appSource, /<ListView[^>]+tasks=\{visibleTasks\}/);
  assert.match(appSource, /<BoardView[^>]+tasks=\{visibleTasks\}/);
});

test("search filters only the supplied view and matches task and project details", () => {
  const projects = [{ id: "project-1", name: "MCP Todo", color: "#fff" }];
  const tasks = [
    { ...makeTask(), id: "title", title: "Release check" },
    { ...makeTask(), id: "tag", title: "Other", tags: ["联调"] },
  ];

  assert.deepEqual(searchTasks(tasks, projects, "mcp").map((task) => task.id), ["title", "tag"]);
  assert.deepEqual(searchTasks(tasks.slice(1), projects, "release"), []);
  assert.deepEqual(searchTasks(tasks, projects, "联调").map((task) => task.id), ["tag"]);
});
