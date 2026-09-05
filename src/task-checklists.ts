import type { AcceptanceCriterion, Subtask, Task } from "./types";

// Textarea lines have no stable IDs. Only unchanged titles can safely retain state;
// a renamed line is new, and duplicate titles consume old entries once, in order.
function reconcileChecklist(existing: Subtask[], titles: string[]) {
  const used = new Set<number>();
  return titles.map((title) => {
    const match = existing.findIndex((item, index) => !used.has(index) && item.title === title);
    if (match < 0) return { id: crypto.randomUUID(), title, completed: false };
    used.add(match);
    return { ...existing[match] };
  });
}

export function reconcileSubtasks(existing: Subtask[], titles: string[]) {
  return reconcileChecklist(existing, titles);
}

export function reconcileAcceptanceCriteria(existing: AcceptanceCriterion[], titles: string[]) {
  return reconcileChecklist(existing, titles);
}

export function checklistEditsOnLatest(task: Task, edits: Partial<Task>): Partial<Task> {
  const applyTitles = (latest: Subtask[], edited: Subtask[]) => edited.map((item) => ({
    ...item,
    // The editor changes text and order, never checkbox state. Preserve changes
    // made in the detail panel or by MCP while this save was in flight.
    completed: latest.find((current) => current.id === item.id && current.title === item.title)?.completed ?? false,
  }));
  return {
    ...edits,
    ...(edits.subtasks ? { subtasks: applyTitles(task.subtasks, edits.subtasks) } : {}),
    ...(edits.acceptanceCriteria ? { acceptanceCriteria: applyTitles(task.acceptanceCriteria, edits.acceptanceCriteria) } : {}),
  };
}
