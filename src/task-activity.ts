import type { ActivityItem, Task } from "./types";
import { isValidIsoDate, taskDueLabel } from "./date-utils.ts";
import { validateTaskCompletion } from "./task-validation.ts";

export const MAX_TASK_ACTIVITY_ITEMS = 100;
export const DEFAULT_VISIBLE_ACTIVITY_ITEMS = 4;

const harmlessLegacyActions = new Set([
  "创建任务", "更新子任务", "更新验收标准", "置顶到桌面", "取消桌面置顶",
  "归档任务", "恢复归档任务", "批量归档任务", "项目删除前转移任务",
  "添加图片", "添加附件", "移除图片", "移除附件",
]);
const completionActions = new Set(["完成任务", "状态更新为已完成", "从便签完成任务"]);
const offsetDateTime = /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d:\d\d)$/;

export function isCompletionTimestamp(value: string) {
  return offsetDateTime.test(value) && isValidIsoDate(value.slice(0, 10))
    && Number(value.slice(11, 13)) < 24 && !Number.isNaN(Date.parse(value));
}

export function completionTime(task: Pick<Task, "status" | "completedAt" | "activity">): string | null {
  if (task.status !== "done") return null;
  if (task.completedAt) return isCompletionTimestamp(task.completedAt) ? task.completedAt : null;
  for (const item of [...task.activity].reverse()) {
    if (completionActions.has(item.action)) {
      return isCompletionTimestamp(item.at) ? item.at : null;
    }
    if (!harmlessLegacyActions.has(item.action)) return null;
  }
  return null;
}

export function completionDate(value: string) {
  const date = new Date(value);
  const part = (number: number) => String(number).padStart(2, "0");
  return `${date.getFullYear()}-${part(date.getMonth() + 1)}-${part(date.getDate())}`;
}

export function completionDateTime(value: string) {
  const date = new Date(value);
  const part = (number: number) => String(number).padStart(2, "0");
  return `${completionDate(value)} ${part(date.getHours())}:${part(date.getMinutes())}:${part(date.getSeconds())}`;
}

export function taskDateLabel(task: Task, today = new Date()) {
  if (task.status === "done") {
    const time = completionTime(task);
    return time ? `${completionDate(time)} 完成` : "已完成（时间未记录）";
  }
  return taskDueLabel(task.dueDate, task.dueLabel, today);
}

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
  const nextStatus = patch.status ?? task.status;
  const completedAt = nextStatus !== "done" ? null
    : task.status === "done" ? completionTime(task) : now.toISOString();
  validateTaskCompletion(task, { ...task, ...patch });
  return {
    ...task,
    ...patch,
    completedAt,
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
