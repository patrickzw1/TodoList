import type { ActivityItem, Task } from "./types";
import { validateTaskCompletion } from "./task-validation.ts";

export const MAX_TASK_ACTIVITY_ITEMS = 100;
export const DEFAULT_VISIBLE_ACTIVITY_ITEMS = 4;

export function boundedActivity(items: ActivityItem[]) {
  return items.length > MAX_TASK_ACTIVITY_ITEMS
    ? items.slice(-MAX_TASK_ACTIVITY_ITEMS)
    : items;
}

export function activityItemsForDetail(items: ActivityItem[], expanded: boolean) {
  const visibleItems = expanded ? items : items.slice(-DEFAULT_VISIBLE_ACTIVITY_ITEMS);
  return [...visibleItems].reverse();
}

export function taskWithUserActivity(task: Task, action: string, patch: Partial<Task>, now = new Date()): Task {
  validateTaskCompletion(task, { ...task, ...patch });
  return {
    ...task,
    ...patch,
    version: task.version + 1,
    activity: boundedActivity([
      ...task.activity,
      { id: crypto.randomUUID(), action, actor: "user", at: now.toISOString() },
    ]),
  };
}

export function formatActivityTime(value: string) {
  if (value === "刚刚") return "时间未记录";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}
