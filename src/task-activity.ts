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

const creationActions = new Set(["创建任务", "Codex 创建任务", "Claude Code 创建任务", "DeepSeek Harness 创建任务", "AI 创建任务"]);
const mutationActions = new Set([...creationActions, ...harmlessLegacyActions, ...completionActions,
  "编辑任务", "重新打开任务", "取消验收并重新打开任务", "状态更新为待开始", "状态更新为进行中", "状态更新为已阻塞",
  "Codex 更新任务", "Claude Code 更新任务", "DeepSeek Harness 更新任务", "AI 更新任务",
  ...["Codex", "Claude Code", "DeepSeek Harness", "AI"].flatMap((client) =>
    ["添加附件", "添加图片", "移除托管文件关联"].map((action) => `${client} ${action}`)),
]);

export function taskAuditTimes(task: Pick<Task, "createdAt" | "updatedAt" | "activity">) {
  const valid = (value?: string | null) => value && isCompletionTimestamp(value) ? value : null;
  const creation = task.activity.find((item) => creationActions.has(item.action) && valid(item.at));
  const latest = task.activity.at(-1);
  return { createdAt: task.createdAt !== undefined ? valid(task.createdAt) : creation?.at ?? null,
    updatedAt: task.updatedAt !== undefined ? valid(task.updatedAt) : (latest && mutationActions.has(latest.action) ? valid(latest.at) : null) };
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
  if (Object.entries(patch).every(([key, value]) => JSON.stringify(task[key as keyof Task]) === JSON.stringify(value))) return task;
  const nextStatus = patch.status ?? task.status;
  const completedAt = nextStatus !== "done" ? null
    : task.status === "done" ? completionTime(task) : now.toISOString();
  validateTaskCompletion(task, { ...task, ...patch });
  return {
    ...task,
    ...patch,
    createdAt: taskAuditTimes(task).createdAt,
    updatedAt: now.toISOString(),
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
