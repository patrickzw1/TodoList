import type { Project, Task } from "./types";

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
  return tasks.filter((task) => !task.archived && task.dueDate <= today);
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
