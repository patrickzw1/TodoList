import type { Task, Workspace } from "./types";

export function validateTaskCompletion(previous: Task | undefined, next: Task) {
  if (next.status !== "done") return;
  const unconfirmed = next.acceptanceCriteria.filter((criterion) => !criterion.completed);
  // Existing legacy inconsistencies must not block unrelated edits. Reject newly
  // completing a task or introducing a new/unconfirmed requirement on a done task.
  if (unconfirmed.some((criterion) => previous?.status !== "done" || !previous.acceptanceCriteria.some(
    (old) => old.id === criterion.id && old.title === criterion.title && !old.completed,
  ))) {
    throw new Error(`任务“${next.title}”尚有未确认的验收标准，请逐项确认后再完成`);
  }
}

export function validateWorkspaceCompletion(previous: Workspace, next: Workspace) {
  const previousTasks = new Map(previous.tasks.map((task) => [task.id, task]));
  for (const task of next.tasks) validateTaskCompletion(previousTasks.get(task.id), task);
}
