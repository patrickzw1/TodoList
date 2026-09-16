import type { AcceptanceCriterion, Subtask, Task } from "./types";

type ChecklistItem = Subtask | AcceptanceCriterion;

export interface ChecklistDraftItem {
  id: string;
  title: string;
}

export interface ChecklistEditorEdit {
  deletedIds: string[];
  titleEdits: Array<Pick<ChecklistItem, "id" | "title">>;
  additions: ChecklistItem[];
}

export interface TaskChecklistEdits {
  subtasks?: ChecklistEditorEdit;
  acceptanceCriteria?: ChecklistEditorEdit;
}

export type TaskEditorPatch = Omit<Partial<Task>, "subtasks" | "acceptanceCriteria"> & TaskChecklistEdits;

export function checklistEditorEdit(existing: ChecklistItem[], draft: ChecklistDraftItem[]) {
  const existingById = new Map(existing.map((item) => [item.id, item]));
  const seen = new Set<string>();
  const normalizedDraft = draft.flatMap((item) => {
    if (seen.has(item.id)) throw new Error(`duplicate checklist item id: ${item.id}`);
    seen.add(item.id);
    const title = item.title.trim();
    if (!title && existingById.has(item.id)) throw new Error(`existing checklist item is empty: ${item.id}`);
    return title ? [{ id: item.id, title }] : [];
  });
  const presentIds = new Set(normalizedDraft.map((item) => item.id));
  const deletedIds = existing.filter((item) => !presentIds.has(item.id)).map((item) => item.id);
  const titleEdits = normalizedDraft.filter((item) => {
    const current = existingById.get(item.id);
    return current && current.title !== item.title;
  });
  const additions = normalizedDraft
    .filter((item) => !existingById.has(item.id))
    .map((item) => ({ ...item, completed: false }));

  if (!deletedIds.length && !titleEdits.length && !additions.length) return undefined;
  return { deletedIds, titleEdits, additions } satisfies ChecklistEditorEdit;
}

export function applyChecklistEditorEdit(latest: ChecklistItem[], edit: ChecklistEditorEdit) {
  const deleted = new Set(edit.deletedIds);
  const titles = new Map(edit.titleEdits.map((item) => [item.id, item.title]));
  const result = latest
    .filter((item) => !deleted.has(item.id))
    .map((item) => titles.has(item.id) ? { ...item, title: titles.get(item.id)! } : item);
  const retainedIds = new Set(result.map((item) => item.id));
  for (const addition of edit.additions) {
    if (!retainedIds.has(addition.id)) {
      result.push({ ...addition });
      retainedIds.add(addition.id);
    }
  }
  return result;
}

export function checklistEditsOnLatest(task: Task, edits: TaskEditorPatch): Partial<Task> {
  const { subtasks, acceptanceCriteria, ...taskEdits } = edits;
  return {
    ...taskEdits,
    ...(subtasks ? { subtasks: applyChecklistEditorEdit(task.subtasks, subtasks) } : {}),
    ...(acceptanceCriteria ? { acceptanceCriteria: applyChecklistEditorEdit(task.acceptanceCriteria, acceptanceCriteria) } : {}),
  };
}
