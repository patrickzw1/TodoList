import { boundedActivity } from "./task-activity.ts";
import type { AcceptanceCriterion, ActivityItem, ManagedFile, Project, Subtask, Task, Workspace } from "./types";

export const BACKUP_SCHEMA_VERSION = 2;
const LEGACY_BACKUP_SCHEMA_VERSION = 1;
export const MAX_BACKUP_BYTES = 25 * 1024 * 1024;

export interface WorkspaceBackup {
  schemaVersion: number;
  exportedAt: number;
  appVersion: string;
  workspace: Workspace;
  managedFiles: ManagedFilePayload[];
}

export interface ManagedFilePayload {
  storageKey: string;
  data: string;
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label}格式不正确`);
  return value as Record<string, unknown>;
}

function text(value: unknown, label: string) {
  if (typeof value !== "string") throw new Error(`${label}格式不正确`);
  return value;
}

function integer(value: unknown, label: string) {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error(`${label}格式不正确`);
  return value;
}

function boolean(value: unknown, label: string) {
  if (typeof value !== "boolean") throw new Error(`${label}格式不正确`);
  return value;
}

function array(value: unknown, label: string) {
  if (!Array.isArray(value)) throw new Error(`${label}格式不正确`);
  return value;
}

function stringArray(value: unknown, label: string) {
  return array(value, label).map((item) => text(item, label));
}

function optionalArray(value: unknown, label: string) {
  return value === undefined ? [] : array(value, label);
}

function choice<T extends string>(value: unknown, choices: readonly T[], label: string): T {
  const result = text(value, label);
  if (!choices.includes(result as T)) throw new Error(`${label}包含不支持的值`);
  return result as T;
}

function projectFrom(value: unknown, index: number): Project {
  const item = record(value, `项目 ${index + 1}`);
  const project = {
    id: text(item.id, "项目 ID"),
    name: text(item.name, "项目名称"),
    color: text(item.color, "项目颜色"),
  };
  if (!project.id.trim() || !project.name.trim()) throw new Error("备份中存在空的项目 ID 或名称");
  return project;
}

function subtaskFrom(value: unknown): Subtask {
  const item = record(value, "子任务");
  return { id: text(item.id, "子任务 ID"), title: text(item.title, "子任务标题"), completed: boolean(item.completed, "子任务状态") };
}

function criterionFrom(value: unknown): AcceptanceCriterion {
  const item = record(value, "验收标准");
  return { id: text(item.id, "验收标准 ID"), title: text(item.title, "验收标准标题"), completed: boolean(item.completed, "验收标准状态") };
}

function activityFrom(value: unknown): ActivityItem {
  const item = record(value, "活动记录");
  return {
    id: text(item.id, "活动记录 ID"),
    action: text(item.action, "活动内容"),
    actor: choice(item.actor, ["user", "codex"] as const, "活动操作者"),
    at: text(item.at, "活动时间"),
  };
}

function managedFileFrom(value: unknown): ManagedFile {
  const item = record(value, "托管文件");
  return {
    id: text(item.id, "托管文件 ID"),
    originalName: text(item.originalName, "托管文件名"),
    mediaType: text(item.mediaType, "托管文件类型"),
    size: integer(item.size, "托管文件大小"),
    storageKey: text(item.storageKey, "托管文件键"),
    addedAt: text(item.addedAt, "托管文件添加时间"),
  };
}

function taskFrom(value: unknown, index: number): Task {
  const item = record(value, `任务 ${index + 1}`);
  const task: Task = {
    id: text(item.id, "任务 ID"),
    projectId: text(item.projectId, "任务项目 ID"),
    title: text(item.title, "任务标题"),
    description: text(item.description, "任务描述"),
    status: choice(item.status, ["todo", "in_progress", "blocked", "done"] as const, "任务状态"),
    priority: choice(item.priority, ["low", "medium", "high"] as const, "任务优先级"),
    dueLabel: text(item.dueLabel, "任务日期标签"),
    dueDate: text(item.dueDate, "任务日期"),
    tags: stringArray(item.tags, "任务标签"),
    source: choice(item.source, ["手动创建", "Codex 创建"] as const, "任务来源"),
    archived: boolean(item.archived, "任务归档状态"),
    pinned: boolean(item.pinned, "任务置顶状态"),
    version: integer(item.version, "任务版本"),
    subtasks: array(item.subtasks, "子任务").map(subtaskFrom),
    acceptanceCriteria: array(item.acceptanceCriteria, "验收标准").map(criterionFrom),
    attachments: optionalArray(item.attachments, "附件").map(managedFileFrom),
    images: optionalArray(item.images, "图片").map(managedFileFrom),
    dependencies: stringArray(item.dependencies, "任务依赖"),
    activity: boundedActivity(array(item.activity, "活动记录").map(activityFrom)),
  };
  if (!task.id.trim() || !task.title.trim()) throw new Error("备份中存在空的任务 ID 或标题");
  return task;
}

export function validateWorkspace(value: unknown): Workspace {
  const item = record(value, "任务库");
  const projects = array(item.projects, "项目列表").map(projectFrom);
  const tasks = array(item.tasks, "任务列表").map(taskFrom);
  const projectIds = new Set(projects.map((project) => project.id));
  if (projectIds.size !== projects.length) throw new Error("备份中存在重复的项目 ID");
  const taskIds = new Set(tasks.map((task) => task.id));
  if (taskIds.size !== tasks.length) throw new Error("备份中存在重复的任务 ID");
  const orphan = tasks.find((task) => !projectIds.has(task.projectId));
  if (orphan) throw new Error(`任务“${orphan.title}”引用了不存在的项目`);
  return { version: integer(item.version, "任务库版本"), projects, tasks };
}

export function createWorkspaceBackup(workspace: Workspace, exportedAt = Math.floor(Date.now() / 1000), appVersion = "0.2.2"): WorkspaceBackup {
  return {
    schemaVersion: BACKUP_SCHEMA_VERSION,
    exportedAt,
    appVersion,
    workspace: validateWorkspace(structuredClone(workspace)),
    managedFiles: [],
  };
}

export function backupJson(backup: WorkspaceBackup) {
  const content = JSON.stringify(backup, null, 2);
  if (new TextEncoder().encode(content).byteLength > MAX_BACKUP_BYTES) throw new Error("备份超过 25 MB 安全上限");
  return content;
}

export function parseWorkspaceBackup(content: string): WorkspaceBackup {
  if (new TextEncoder().encode(content).byteLength > MAX_BACKUP_BYTES) throw new Error("备份超过 25 MB 安全上限");
  let value: unknown;
  try {
    value = JSON.parse(content);
  } catch {
    throw new Error("文件不是有效的 TodoList JSON 备份");
  }
  const item = record(value, "备份文件");
  const schemaVersion = integer(item.schemaVersion, "备份格式版本");
  if (schemaVersion !== BACKUP_SCHEMA_VERSION && schemaVersion !== LEGACY_BACKUP_SCHEMA_VERSION) throw new Error(`不支持备份格式版本 ${schemaVersion}，当前支持版本 ${LEGACY_BACKUP_SCHEMA_VERSION} 和 ${BACKUP_SCHEMA_VERSION}`);
  return {
    schemaVersion,
    exportedAt: integer(item.exportedAt, "备份时间"),
    appVersion: text(item.appVersion, "应用版本"),
    workspace: validateWorkspace(item.workspace),
    managedFiles: optionalArray(item.managedFiles, "托管文件内容").map((value) => {
      const file = record(value, "托管文件内容");
      return { storageKey: text(file.storageKey, "托管文件键"), data: text(file.data, "托管文件内容") };
    }),
  };
}

export function remapManagedFileStorageKeys(workspace: Workspace, mappings: { originalStorageKey: string; newStorageKey: string }[]) {
  const map = new Map(mappings.map((mapping) => [mapping.originalStorageKey, mapping.newStorageKey]));
  return {
    ...workspace,
    tasks: workspace.tasks.map((task) => ({
      ...task,
      attachments: task.attachments.map((file) => ({ ...file, storageKey: map.get(file.storageKey) ?? file.storageKey })),
      images: task.images.map((file) => ({ ...file, storageKey: map.get(file.storageKey) ?? file.storageKey })),
    })),
  };
}

export function backupFileName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function importedWorkspaceForSave(imported: Workspace, current: Workspace) {
  let taskVersion = Math.max(current.version, imported.version);
  for (const task of [...current.tasks, ...imported.tasks]) taskVersion = Math.max(taskVersion, task.version);
  if (!Number.isSafeInteger(taskVersion + 1) || !Number.isSafeInteger(current.version + 1)) throw new Error("任务版本已达到安全上限，无法恢复备份");
  return {
    ...structuredClone(imported), version: current.version + 1,
    tasks: imported.tasks.map((task) => ({ ...structuredClone(task), version: taskVersion + 1 })),
  };
}
