import { taskWithUserActivity } from "./task-activity.ts";
import type { ManagedFile, Project, Task, Workspace } from "./types";

function changedWorkspace(workspace: Workspace, tasks: Task[], projects = workspace.projects): Workspace {
  return { ...workspace, version: workspace.version + 1, projects, tasks };
}

export function archiveTasksById(workspace: Workspace, taskIds: Iterable<string>) {
  const selected = new Set(taskIds);
  return changedWorkspace(workspace, workspace.tasks.map((task) => selected.has(task.id) && !task.archived
    ? taskWithUserActivity(task, "批量归档任务", { archived: true, pinned: false })
    : task));
}

export function deleteArchivedTasksById(workspace: Workspace, taskIds: Iterable<string>) {
  const selected = new Set(taskIds);
  return changedWorkspace(workspace, workspace.tasks.filter((task) => !(selected.has(task.id) && task.archived)));
}

export function moveProjectTasks(
  workspace: Workspace,
  sourceProjectId: string,
  targetProjectId: string,
) {
  if (sourceProjectId === targetProjectId) throw new Error("承接项目不能是待删除项目");
  if (!workspace.projects.some((project) => project.id === targetProjectId)) throw new Error("承接项目不存在");
  const projects = workspace.projects.filter((project) => project.id !== sourceProjectId);
  const tasks = workspace.tasks.map((task) => task.projectId === sourceProjectId
    ? taskWithUserActivity(task, "项目删除前转移任务", { projectId: targetProjectId })
    : task);
  return changedWorkspace(workspace, tasks, projects);
}

export function moveProjectTasksToNewProject(
  workspace: Workspace,
  sourceProjectId: string,
  target: Project,
) {
  if (workspace.projects.some((project) => project.id === target.id)) throw new Error("承接项目 ID 已存在");
  if (workspace.projects.some((project) => project.name.trim().toLocaleLowerCase() === target.name.trim().toLocaleLowerCase())) {
    throw new Error("已经存在同名项目");
  }
  const projects = [...workspace.projects.filter((project) => project.id !== sourceProjectId), target];
  const tasks = workspace.tasks.map((task) => task.projectId === sourceProjectId
    ? taskWithUserActivity(task, "项目删除前转移任务", { projectId: target.id })
    : task);
  return changedWorkspace(workspace, tasks, projects);
}

export function deleteProjectWithTasks(workspace: Workspace, projectId: string) {
  return changedWorkspace(
    workspace,
    workspace.tasks.filter((task) => task.projectId !== projectId),
    workspace.projects.filter((project) => project.id !== projectId),
  );
}

export function addManagedFile(workspace: Workspace, taskId: string, kind: "attachments" | "images", file: ManagedFile) {
  return changedWorkspace(workspace, workspace.tasks.map((task) => {
    if (task.id !== taskId || task[kind].some((item) => item.id === file.id)) return task;
    return taskWithUserActivity(task, kind === "images" ? "添加图片" : "添加附件", { [kind]: [...task[kind], file] });
  }));
}

export function removeManagedFile(workspace: Workspace, taskId: string, kind: "attachments" | "images", fileId: string) {
  return changedWorkspace(workspace, workspace.tasks.map((task) => task.id === taskId
    ? taskWithUserActivity(task, kind === "images" ? "移除图片" : "移除附件", { [kind]: task[kind].filter((item) => item.id !== fileId) })
    : task));
}

export function reconcileSelectedTaskIds(selected: Iterable<string>, visibleTaskIds: Iterable<string>) {
  const visible = new Set(visibleTaskIds);
  return new Set([...selected].filter((taskId) => visible.has(taskId)));
}
