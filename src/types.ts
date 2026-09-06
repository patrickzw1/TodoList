export type TaskStatus = "todo" | "in_progress" | "blocked" | "done";
export type Priority = "low" | "medium" | "high";

export interface Project {
  id: string;
  name: string;
  color: string;
}

export interface Subtask {
  id: string;
  title: string;
  completed: boolean;
}

export interface AcceptanceCriterion {
  id: string;
  title: string;
  completed: boolean;
}

export interface ActivityItem {
  id: string;
  action: string;
  actor: "user" | "codex";
  at: string;
}

export interface ManagedFile {
  id: string;
  originalName: string;
  mediaType: string;
  size: number;
  storageKey: string;
  addedAt: string;
}

export interface Task {
  id: string;
  projectId: string;
  title: string;
  description: string;
  status: TaskStatus;
  priority: Priority;
  dueLabel: string;
  dueDate: string;
  tags: string[];
  source: "手动创建" | "Codex 创建";
  archived: boolean;
  pinned: boolean;
  version: number;
  subtasks: Subtask[];
  acceptanceCriteria: AcceptanceCriterion[];
  attachments: ManagedFile[];
  images: ManagedFile[];
  dependencies: string[];
  activity: ActivityItem[];
}

export interface Workspace {
  version: number;
  projects: Project[];
  tasks: Task[];
}
