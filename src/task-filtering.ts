import type { Project, Task } from "./types";
import { isDueDateOnOrBefore, isValidIsoDate, parseIsoDate, taskDueLabel, UNSCHEDULED_DUE_DATE } from "./date-utils.ts";
import { completionDate, completionDateTime, completionTime, taskAuditTimes } from "./task-activity.ts";

export type TaskSortKey = "manual" | "project" | "createdAt" | "dueDate" | "completedAt" | "updatedAt";
export type TaskSort = { key: TaskSortKey; direction: "asc" | "desc" };
export const taskSortLabels: Record<TaskSortKey, string> = { manual: "手动顺序", project: "按项目分组", createdAt: "创建时间", dueDate: "截止日期", completedAt: "完成时间", updatedAt: "更新时间" };
export const defaultTaskSort: TaskSort = { key: "manual", direction: "asc" };

export function parseTaskSort(value: string | null): TaskSort {
  try {
    const sort = JSON.parse(value ?? "null") as TaskSort | null;
    if (sort && Object.hasOwn(taskSortLabels, sort.key) && (sort.direction === "asc" || sort.direction === "desc")) return sort;
  } catch { /* Invalid preferences never prevent loading tasks. */ }
  return defaultTaskSort;
}

export function taskSortTime(task: Task, key: TaskSortKey): string | null {
  if (key === "dueDate") return task.dueDate !== UNSCHEDULED_DUE_DATE && isValidIsoDate(task.dueDate) ? task.dueDate : null;
  if (key === "completedAt") return completionTime(task);
  if (key === "createdAt" || key === "updatedAt") return taskAuditTimes(task)[key];
  return null;
}

export function calendarDateLabel(date: string, today = new Date()) {
  const parts = date === UNSCHEDULED_DUE_DATE ? null : parseIsoDate(date);
  if (!parts) return null;
  return `${parts.year === today.getFullYear() ? "" : `${parts.year}年`}${parts.month}月${parts.day}日`;
}

export function taskListDate(task: Task, sort: TaskSortKey, today = new Date()) {
  const completedAt = completionTime(task);
  const completed = task.status === "done"
    ? completedAt ? `${calendarDateLabel(completionDate(completedAt), today)}完成` : "已完成（时间未记录）"
    : null;
  const deadline = calendarDateLabel(task.dueDate, today);
  if (sort === "dueDate") {
    // The primary date must be the one used to sort, even on a completed row.
    return { primary: deadline ?? "未安排", completed };
  }
  if (sort === "manual" || sort === "project") {
    return { primary: completed ?? deadline ?? taskDueLabel(task.dueDate, task.dueLabel, today), completed: null };
  }
  const time = taskSortTime(task, sort);
  return { primary: time ? completionDateTime(time).slice(0, 16) : "时间未记录", completed: null };
}

export function sortTasks(tasks: Task[], projects: Project[], sort: TaskSort): Task[] {
  if (sort.key === "manual") return tasks;
  const projectOrder = new Map(projects.map((project, index) => [project.id, index]));
  return tasks.map((task, index) => ({ task, index })).sort((a, b) => {
    if (sort.key === "project") return (projectOrder.get(a.task.projectId) ?? Infinity) - (projectOrder.get(b.task.projectId) ?? Infinity) || a.index - b.index;
    const aTime = taskSortTime(a.task, sort.key), bTime = taskSortTime(b.task, sort.key);
    if (!aTime || !bTime) return (aTime ? -1 : bTime ? 1 : 0) || a.index - b.index;
    const value = sort.key === "dueDate" ? aTime.localeCompare(bTime) : Date.parse(aTime) - Date.parse(bTime);
    return value * (sort.direction === "asc" ? 1 : -1) || a.index - b.index;
  }).map(({ task }) => task);
}

export type TaskListView =
  | { kind: "today" }
  | { kind: "all" }
  | { kind: "active" }
  | { kind: "archived" }
  | { kind: "project"; projectId: string };

export function tasksForView(tasks: Task[], view: TaskListView, today: string) {
  if (view.kind === "all") return tasks.filter((task) => !task.archived);
  if (view.kind === "archived") return tasks.filter((task) => task.archived);
  if (view.kind === "active") return tasks.filter((task) => !task.archived && task.status === "in_progress");
  if (view.kind === "project") return tasks.filter((task) => !task.archived && task.projectId === view.projectId);
  return tasks.filter((task) => !task.archived && isDueDateOnOrBefore(task.dueDate, today));
}

export function searchTasks(tasks: Task[], projects: Project[], searchQuery: string) {
  const query = searchQuery.trim().toLocaleLowerCase();
  if (!query) return tasks;
  return tasks.filter((task) => {
    const projectName = projects.find((project) => project.id === task.projectId)?.name ?? "";
    return [
      task.title,
      task.description,
      projectName,
      ...task.tags,
      ...task.acceptanceCriteria.map((criterion) => criterion.title),
    ]
      .join("\n")
      .toLocaleLowerCase()
      .includes(query);
  });
}
