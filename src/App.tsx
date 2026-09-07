import {
  Archive, ArrowCounterClockwise, ArrowsInSimple, ArrowsOutSimple, ArrowUp, CaretDown, CheckCircle, Circle, Columns, DotsSixVertical, Folder, Gear, LinkSimple,
  List, LockKey, MagnifyingGlass, PencilSimple, Play, Plus, PushPin, PushPinSlash, Sun, Trash, X,
} from "@phosphor-icons/react";
import { isTauri, invoke } from "@tauri-apps/api/core";
import { FormEvent, type PointerEvent as ReactPointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Project, Task, TaskStatus, Workspace } from "./types";
import { CodexIntegrationCard, readCodexIntegrationStatus, type CodexIntegrationStatus } from "./CodexIntegrationCard";
import { DataBackupCard } from "./DataBackupCard";
import { TaskFileSections } from "./TaskFiles";
import { SoftwareUpdateCard, SoftwareUpdateProvider, useSoftwareUpdate } from "./SoftwareUpdateCard";
import { isOverdue, localIsoDate, taskDueLabel, todayHeading } from "./date-utils";
import { activityItemsForDetail, formatActivityTime, taskWithUserActivity } from "./task-activity";
import { resolveDefaultProjectId } from "./task-creation";
import { searchTasks, tasksForView, type TaskListView } from "./task-filtering";
import { ConfirmDialog, NoticeDialog, ProjectDeletionDialog, ProjectEditorDialog, TaskEditorDialog, type TaskEdits } from "./dialogs";
import { closeCurrentWindow, openMainWindow, openStickyWindow } from "./window-actions";
import { useWorkspace, type StorageState } from "./workspace-store";
import { importedWorkspaceForSave, remapManagedFileStorageKeys, type WorkspaceBackup } from "./workspace-backup";
import { useAutoHideScrollbar } from "./use-auto-hide-scrollbar";
import { validateTaskCompletion } from "./task-validation";
import { checklistEditsOnLatest } from "./task-checklists";
import {
  addManagedFile, archiveTasksById, deleteArchivedTasksById, deleteProjectWithTasks, moveProjectTasks,
  moveProjectTasksToNewProject, reconcileSelectedTaskIds, removeManagedFile, reorderTaskSubset,
} from "./workspace-actions";

type View =
  | TaskListView
  | { kind: "integration" }
  | { kind: "settings" };

type PendingDeletion =
  | { kind: "task"; taskId: string }
  | { kind: "project"; projectId: string }
  | { kind: "batch"; taskIds: string[] }
  | null;

const statusLabel: Record<TaskStatus, string> = {
  todo: "待开始",
  in_progress: "进行中",
  blocked: "已阻塞",
  done: "已完成",
};
const statusOrder: TaskStatus[] = ["todo", "in_progress", "blocked", "done"];
const DETAIL_TRANSITION_MS = 220;
const BOARD_DRAG_THRESHOLD = 6;
const LIST_DRAG_THRESHOLD = 5;
const LIST_EDGE_ZONE = 52;
const LIST_MAX_SCROLL = 15;

function nextWorkspace(workspace: Workspace, tasks: Task[]): Workspace {
  return { ...workspace, version: workspace.version + 1, tasks };
}

function useCurrentDate() {
  const [current, setCurrent] = useState(() => new Date());
  useEffect(() => {
    const timer = window.setInterval(() => {
      const next = new Date();
      setCurrent((previous) => localIsoDate(previous) === localIsoDate(next) ? previous : next);
    }, 60_000);
    return () => window.clearInterval(timer);
  }, []);
  return current;
}

function StatusButton({ task, onToggle }: { task: Task; onToggle: () => void }) {
  const completed = task.status === "done";
  return (
    <button
      className={`status-button ${completed ? "is-done" : ""} ${task.status === "blocked" ? "is-blocked" : ""}`}
      onClick={(event) => { event.stopPropagation(); onToggle(); }}
      aria-label={completed ? "重新打开任务" : "完成任务"}
      title={completed ? "重新打开" : "标记完成"}
    >
      {completed ? <CheckCircle weight="fill" /> : <Circle />}
    </button>
  );
}

function Sidebar({ projects, view, currentVersion, updateAvailable, storageState, storageMessage, integrationStatus, integrationError, openingSticky, onOpenSticky, onView, onCreate, onCreateProject, onEditProject }: {
  projects: Project[]; view: View; currentVersion: string; updateAvailable: boolean; storageState: StorageState; storageMessage: string; integrationStatus: CodexIntegrationStatus | null; integrationError: string; openingSticky: boolean; onOpenSticky: () => void; onView: (view: View) => void; onCreate: () => void; onCreateProject: () => void; onEditProject: (projectId: string) => void;
}) {
  const [projectsExpanded, setProjectsExpanded] = useState(true);
  const integrationCopy = integrationError
    ? { state: "error", label: "Codex 集成状态读取失败" }
    : integrationStatus?.state === "development"
      ? { state: "development", label: "开发 MCP · todolist_dev" }
      : integrationStatus?.state === "configured"
        ? { state: "configured", label: "Codex 集成已配置" }
        : integrationStatus?.state === "partial"
          ? { state: "partial", label: "Codex 集成待修复" }
          : integrationStatus?.state === "conflict"
            ? { state: "conflict", label: "Codex 集成存在冲突" }
            : integrationStatus
              ? { state: "not-configured", label: integrationStatus.canConfigure ? "Codex 集成未配置" : "网页预览 · 集成不可用" }
              : { state: "checking", label: "正在检查 Codex 集成" };
  return (
    <aside className="sidebar">
      <div className="brand"><CheckCircle weight="bold" /><span>任务台</span></div>
      <button className="primary-create" onClick={onCreate}><Plus weight="bold" />新建任务</button>
      <nav className="main-nav" aria-label="主要导航">
        <button className={view.kind === "today" ? "active" : ""} onClick={() => onView({ kind: "today" })}><Sun />今日</button>
        <button className={view.kind === "all" ? "active" : ""} onClick={() => onView({ kind: "all" })}><List />全部</button>
        <button className={view.kind === "active" ? "active" : ""} onClick={() => onView({ kind: "active" })}><Play />进行中</button>
        <button className={view.kind === "archived" ? "active" : ""} onClick={() => onView({ kind: "archived" })}><Archive />已归档</button>
      </nav>
      <div className="sidebar-rule" />
      <div className="projects-region">
        <div className="projects-heading"><button className="projects-toggle" onClick={() => setProjectsExpanded((expanded) => !expanded)} aria-expanded={projectsExpanded}><CaretDown weight="fill" />项目</button><button onClick={onCreateProject} aria-label="新建项目"><Plus /></button></div>
        <nav className={`project-nav ${projectsExpanded ? "" : "collapsed"}`} aria-label="项目">
          {projects.map((project) => (
            <div className={`project-nav-row ${view.kind === "project" && view.projectId === project.id ? "active" : ""}`} key={project.id}>
              <button className="project-open" onClick={() => onView({ kind: "project", projectId: project.id })}><Folder style={{ color: project.color }} /><span>{project.name}</span></button>
              <button className="project-edit" onClick={() => onEditProject(project.id)} aria-label={`编辑项目 ${project.name}`} title="编辑项目"><PencilSimple /></button>
            </div>
          ))}
        </nav>
      </div>
      <div className="sidebar-bottom">
        <button disabled={openingSticky} onClick={onOpenSticky} title="打开桌面便签并最小化任务台"><PushPin />{openingSticky ? "正在打开便签……" : "桌面便签"}</button>
        <button className={view.kind === "integration" ? "active" : ""} onClick={() => onView({ kind: "integration" })}><LinkSimple />连接与权限</button>
        <button className={`settings-nav ${view.kind === "settings" ? "active" : ""}`} onClick={() => onView({ kind: "settings" })}><Gear /><span>设置</span><small>v{currentVersion}</small>{updateAvailable && <i>可更新</i>}</button>
        <div className={`local-state ${storageState}`} title={storageMessage}><span className="saved-dot" />{storageMessage}</div>
        <div className={`mcp-state ${integrationCopy.state}`} title={integrationError || integrationStatus?.message}><LinkSimple />{integrationCopy.label}</div>
      </div>
    </aside>
  );
}

function TaskRow({ task, project, selected, multiSelected, dragging, dropPosition, onSelect, onToggle, onTogglePin, onMultiSelect, onReorderPointerDown, onReorderPointerMove, onReorderPointerEnd, onReorderPointerLost, onReorderKeyDown }: {
  task: Task; project: Project; selected: boolean; multiSelected: boolean; dragging: boolean; dropPosition: "before" | "after" | null; onSelect: () => void; onToggle: () => void; onTogglePin: () => void; onMultiSelect: () => void;
  onReorderPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onReorderPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onReorderPointerEnd: (event: ReactPointerEvent<HTMLButtonElement>, cancelled?: boolean) => void;
  onReorderPointerLost: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onReorderKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>) => void;
}) {
  const dueLabel = taskDueLabel(task.dueDate, task.dueLabel);
  return (
    <div className={`task-row ${selected ? "selected" : ""} ${multiSelected ? "multi-selected" : ""} ${dragging ? "is-reordering" : ""} ${dropPosition ? `drop-${dropPosition}` : ""}`} data-task-id={task.id} data-project-id={task.projectId} data-archived={String(task.archived)} role="button" tabIndex={0} onClick={onSelect} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onSelect(); }}>
      <button
        type="button"
        className="task-reorder-handle"
        aria-label={`移动任务 ${task.title}`}
        title="拖动排序；也可用上下方向键"
        onClick={(event) => event.stopPropagation()}
        onPointerDown={onReorderPointerDown}
        onPointerMove={onReorderPointerMove}
        onPointerUp={(event) => onReorderPointerEnd(event)}
        onPointerCancel={(event) => onReorderPointerEnd(event, true)}
        onLostPointerCapture={onReorderPointerLost}
        onKeyDown={onReorderKeyDown}
      ><DotsSixVertical weight="bold" /></button>
      <input className="task-multi-checkbox" type="checkbox" checked={multiSelected} onClick={(event) => event.stopPropagation()} onChange={onMultiSelect} aria-label={`选择任务 ${task.title}`} />
      <StatusButton task={task} onToggle={onToggle} />
      <span className="task-title">{task.title}</span>
      <button
        className={`row-pin-button ${task.pinned ? "is-pinned" : ""}`}
        onClick={(event) => { event.stopPropagation(); onTogglePin(); }}
        aria-label={task.pinned ? "取消桌面置顶" : "置顶到桌面"}
        aria-pressed={task.pinned}
        title={task.pinned ? "取消桌面置顶" : "置顶到桌面"}
      >
        <PushPin weight={task.pinned ? "fill" : "regular"} />
      </button>
      <span className="project-chip"><i className="project-color-dot" style={{ backgroundColor: project.color }} />{project.name}</span>
      <span className={`due-label ${isOverdue(task.dueDate) && task.status !== "done" ? "overdue" : ""}`}>{dueLabel}</span>
    </div>
  );
}

function ListView({ projects, tasks, selectedTaskId, selectedTaskIds, onSelect, onToggle, onTogglePin, onMultiSelect, onSelectAll, onReorder }: {
  projects: Project[]; tasks: Task[]; selectedTaskId: string | null; selectedTaskIds: Set<string>; onSelect: (id: string) => void; onToggle: (id: string) => void; onTogglePin: (id: string) => void; onMultiSelect: (id: string) => void; onSelectAll: (selected: boolean) => void;
  onReorder: (projectId: string, archived: boolean, visibleTaskIds: string[], orderedTaskIds: string[]) => void;
}) {
  const selectAll = useRef<HTMLInputElement>(null);
  const [dragPreview, setDragPreview] = useState<{ taskId: string; title: string; x: number; y: number; width: number } | null>(null);
  const [dropIndicator, setDropIndicator] = useState<{ taskId: string; position: "before" | "after" } | null>(null);
  const dragSession = useRef<{
    taskId: string; projectId: string; archived: boolean; pointerId: number; handle: HTMLButtonElement;
    startX: number; startY: number; lastX: number; lastY: number; width: number; title: string;
    visibleTaskIds: string[]; orderedTaskIds: string[]; scrollContainer: HTMLElement | null; active: boolean; raf: number | null;
  } | null>(null);
  const selectedVisible = tasks.filter((task) => selectedTaskIds.has(task.id)).length;
  const allSelected = tasks.length > 0 && selectedVisible === tasks.length;
  useEffect(() => { if (selectAll.current) selectAll.current.indeterminate = selectedVisible > 0 && !allSelected; }, [allSelected, selectedVisible]);

  const updateDropAtPoint = (x: number, y: number) => {
    const session = dragSession.current;
    if (!session?.active) return;
    const row = document.elementFromPoint(x, y)?.closest<HTMLElement>(".task-row");
    if (!row || row.dataset.projectId !== session.projectId || row.dataset.archived !== String(session.archived)) {
      setDropIndicator(null);
      session.orderedTaskIds = session.visibleTaskIds;
      return;
    }
    const targetId = row.dataset.taskId;
    if (!targetId || !session.visibleTaskIds.includes(targetId)) return;
    const position = y < row.getBoundingClientRect().top + row.getBoundingClientRect().height / 2 ? "before" : "after";
    const ordered = session.visibleTaskIds.filter((id) => id !== session.taskId);
    let insertion = ordered.indexOf(targetId);
    if (targetId === session.taskId) insertion = session.visibleTaskIds.indexOf(session.taskId);
    else if (position === "after") insertion += 1;
    ordered.splice(Math.max(0, Math.min(insertion, ordered.length)), 0, session.taskId);
    session.orderedTaskIds = ordered;
    setDropIndicator({ taskId: targetId, position });
  };

  const autoScrollFrame = () => {
    const session = dragSession.current;
    if (!session?.active) return;
    const container = session.scrollContainer;
    if (container) {
      const bounds = container.getBoundingClientRect();
      let delta = 0;
      if (session.lastY < bounds.top + LIST_EDGE_ZONE) delta = -LIST_MAX_SCROLL * (1 - Math.max(0, session.lastY - bounds.top) / LIST_EDGE_ZONE);
      else if (session.lastY > bounds.bottom - LIST_EDGE_ZONE) delta = LIST_MAX_SCROLL * (1 - Math.max(0, bounds.bottom - session.lastY) / LIST_EDGE_ZONE);
      if (delta) {
        container.scrollTop += delta;
        updateDropAtPoint(session.lastX, session.lastY);
      }
    }
    session.raf = window.requestAnimationFrame(autoScrollFrame);
  };

  const clearDrag = (cancelled: boolean) => {
    const session = dragSession.current;
    if (!session) return;
    dragSession.current = null;
    if (session.raf !== null) window.cancelAnimationFrame(session.raf);
    if (session.handle.hasPointerCapture(session.pointerId)) session.handle.releasePointerCapture(session.pointerId);
    setDragPreview(null);
    setDropIndicator(null);
    if (!cancelled && session.active && session.orderedTaskIds.some((id, index) => id !== session.visibleTaskIds[index])) {
      onReorder(session.projectId, session.archived, session.visibleTaskIds, session.orderedTaskIds);
    }
  };

  useEffect(() => {
    const cancelOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dragSession.current) {
        event.preventDefault();
        clearDrag(true);
      }
    };
    window.addEventListener("keydown", cancelOnEscape);
    return () => {
      window.removeEventListener("keydown", cancelOnEscape);
      clearDrag(true);
    };
  }, []);

  const beginReorder = (event: ReactPointerEvent<HTMLButtonElement>, task: Task, projectTasks: Task[]) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const row = event.currentTarget.closest<HTMLElement>(".task-row")!;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragSession.current = {
      taskId: task.id, projectId: task.projectId, archived: task.archived, pointerId: event.pointerId, handle: event.currentTarget,
      startX: event.clientX, startY: event.clientY, lastX: event.clientX, lastY: event.clientY, width: Math.min(260, row.getBoundingClientRect().width), title: task.title,
      visibleTaskIds: projectTasks.map((item) => item.id), orderedTaskIds: projectTasks.map((item) => item.id), scrollContainer: row.closest<HTMLElement>(".workspace-content"), active: false, raf: null,
    };
  };
  const moveReorder = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const session = dragSession.current;
    if (!session || session.pointerId !== event.pointerId) return;
    session.lastX = event.clientX;
    session.lastY = event.clientY;
    if (!session.active && Math.hypot(event.clientX - session.startX, event.clientY - session.startY) < LIST_DRAG_THRESHOLD) return;
    event.preventDefault();
    session.active = true;
    setDragPreview({ taskId: session.taskId, title: session.title, x: event.clientX, y: event.clientY, width: session.width });
    updateDropAtPoint(event.clientX, event.clientY);
    if (session.raf === null) session.raf = window.requestAnimationFrame(autoScrollFrame);
  };
  const endReorder = (event: ReactPointerEvent<HTMLButtonElement>, cancelled = false) => {
    const session = dragSession.current;
    if (!session || session.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    clearDrag(cancelled);
  };
  const reorderWithKeyboard = (event: React.KeyboardEvent<HTMLButtonElement>, task: Task, projectTasks: Task[]) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    event.stopPropagation();
    const visibleTaskIds = projectTasks.map((item) => item.id);
    const from = visibleTaskIds.indexOf(task.id);
    const to = Math.max(0, Math.min(visibleTaskIds.length - 1, from + (event.key === "ArrowUp" ? -1 : 1)));
    if (from === to) return;
    const orderedTaskIds = [...visibleTaskIds];
    orderedTaskIds.splice(to, 0, orderedTaskIds.splice(from, 1)[0]);
    const handle = event.currentTarget;
    onReorder(task.projectId, task.archived, visibleTaskIds, orderedTaskIds);
    window.setTimeout(() => handle.focus(), 0);
  };

  return (
    <div className={`task-groups ${dragPreview ? "is-reordering" : ""}`}><label className="list-select-all"><input ref={selectAll} type="checkbox" checked={allSelected} onChange={(event) => onSelectAll(event.target.checked)} /><span>{selectedVisible ? `已选择当前结果中的 ${selectedVisible} 项` : "选择当前视图"}</span></label>
      {projects.map((project) => {
        const projectTasks = tasks.filter((task) => task.projectId === project.id);
        if (!projectTasks.length) return null;
        return (
          <section className="task-group" key={project.id}>
            <h2><i className="project-color-dot" style={{ backgroundColor: project.color }} />{project.name}<span>{projectTasks.length}</span></h2>
            <div className="task-table">
              {projectTasks.map((task) => (
                <TaskRow key={task.id} task={task} project={project} selected={selectedTaskId === task.id} multiSelected={selectedTaskIds.has(task.id)} dragging={dragPreview?.taskId === task.id} dropPosition={dropIndicator?.taskId === task.id ? dropIndicator.position : null} onSelect={() => onSelect(task.id)} onToggle={() => onToggle(task.id)} onTogglePin={() => onTogglePin(task.id)} onMultiSelect={() => onMultiSelect(task.id)} onReorderPointerDown={(event) => beginReorder(event, task, projectTasks)} onReorderPointerMove={moveReorder} onReorderPointerEnd={endReorder} onReorderPointerLost={(event) => { if (dragSession.current?.pointerId === event.pointerId) clearDrag(true); }} onReorderKeyDown={(event) => reorderWithKeyboard(event, task, projectTasks)} />
              ))}
            </div>
          </section>
        );
      })}
      {dragPreview && <div className="list-drag-preview" style={{ left: dragPreview.x + 13, top: dragPreview.y + 13, width: dragPreview.width }}><DotsSixVertical weight="bold" /><strong>{dragPreview.title}</strong></div>}
    </div>
  );
}

function BoardColumn({ status, projects, tasks, isDropTarget, draggingTaskId, onSelect, onToggle, onStatusChange, onPointerDown, onPointerMove, onPointerEnd }: {
  status: TaskStatus;
  projects: Project[];
  tasks: Task[];
  isDropTarget: boolean;
  draggingTaskId: string | null;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
  onStatusChange: (id: string, status: TaskStatus) => void;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>, task: Task, project: Project) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerEnd: (event: ReactPointerEvent<HTMLDivElement>, cancelled?: boolean) => void;
}) {
  const scrollbar = useAutoHideScrollbar<HTMLDivElement>();
  const statusTasks = tasks.filter((task) => task.status === status);

  return (
    <section className={`board-column ${isDropTarget ? "drop-target" : ""}`} data-board-status={status}>
      <h3>{statusLabel[status]}<span>{statusTasks.length}</span></h3>
      <div className="board-column-scroll auto-hide-scrollbar" role="region" aria-label={`${statusLabel[status]}任务`} tabIndex={0} {...scrollbar}>
        {statusTasks.map((task) => {
          const project = projects.find((item) => item.id === task.projectId)!;
          return (
            <div className={`board-card ${draggingTaskId === task.id ? "is-dragging" : ""}`} role="button" tabIndex={0} key={task.id} onPointerDown={(event) => onPointerDown(event, task, project)} onPointerMove={onPointerMove} onPointerUp={(event) => onPointerEnd(event)} onPointerCancel={(event) => onPointerEnd(event, true)} onClick={() => onSelect(task.id)} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onSelect(task.id); }}>
              <span className="board-card-title"><StatusButton task={task} onToggle={() => onToggle(task.id)} />{task.title}</span>
              <span className="board-card-meta"><i className="project-color-dot" style={{ backgroundColor: project.color }} />{project.name} · {taskDueLabel(task.dueDate, task.dueLabel)}</span>
              <select className="board-status-select" value={task.status} onClick={(event) => event.stopPropagation()} onChange={(event) => onStatusChange(task.id, event.target.value as TaskStatus)} aria-label={`修改 ${task.title} 的状态`}><option value="todo">待开始</option><option value="in_progress">进行中</option><option value="blocked">已阻塞</option><option value="done">已完成</option></select>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function BoardView({ projects, tasks, onSelect, onToggle, onStatusChange }: {
  projects: Project[]; tasks: Task[]; onSelect: (id: string) => void; onToggle: (id: string) => void; onStatusChange: (id: string, status: TaskStatus) => void;
}) {
  const [dropTarget, setDropTarget] = useState<TaskStatus | null>(null);
  const [dragPreview, setDragPreview] = useState<{ taskId: string; title: string; meta: string; x: number; y: number; width: number } | null>(null);
  const dragSession = useRef<{ taskId: string; pointerId: number; startX: number; startY: number; width: number; title: string; meta: string; active: boolean } | null>(null);
  const suppressClickTaskId = useRef<string | null>(null);

  const statusAtPoint = (x: number, y: number) => {
    const value = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-board-status]")?.dataset.boardStatus;
    return statusOrder.includes(value as TaskStatus) ? value as TaskStatus : null;
  };
  const startDrag = (event: ReactPointerEvent<HTMLDivElement>, task: Task, project: Project) => {
    if (event.button !== 0 || (event.target as Element).closest("button, select")) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragSession.current = { taskId: task.id, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, width: bounds.width, title: task.title, meta: `${project.name} · ${taskDueLabel(task.dueDate, task.dueLabel)}`, active: false };
  };
  const moveDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const session = dragSession.current;
    if (!session || session.pointerId !== event.pointerId) return;
    if (!session.active && Math.hypot(event.clientX - session.startX, event.clientY - session.startY) < BOARD_DRAG_THRESHOLD) return;
    session.active = true;
    event.preventDefault();
    setDropTarget(statusAtPoint(event.clientX, event.clientY));
    setDragPreview({ taskId: session.taskId, title: session.title, meta: session.meta, x: event.clientX, y: event.clientY, width: session.width });
  };
  const finishDrag = (event: ReactPointerEvent<HTMLDivElement>, cancelled = false) => {
    const session = dragSession.current;
    if (!session || session.pointerId !== event.pointerId) return;
    if (session.active) {
      event.preventDefault();
      const taskId = session.taskId;
      suppressClickTaskId.current = taskId;
      window.setTimeout(() => { if (suppressClickTaskId.current === taskId) suppressClickTaskId.current = null; }, 0);
      const status = cancelled ? null : statusAtPoint(event.clientX, event.clientY);
      if (status) onStatusChange(taskId, status);
    }
    dragSession.current = null;
    setDropTarget(null);
    setDragPreview(null);
  };
  return (
    <div className={`board ${dragPreview ? "is-dragging" : ""}`}>
      {statusOrder.map((status) => (
        <BoardColumn
          key={status}
          status={status}
          projects={projects}
          tasks={tasks}
          isDropTarget={dropTarget === status}
          draggingTaskId={dragPreview?.taskId ?? null}
          onSelect={(taskId) => {
            if (suppressClickTaskId.current === taskId) {
              suppressClickTaskId.current = null;
              return;
            }
            onSelect(taskId);
          }}
          onToggle={onToggle}
          onStatusChange={onStatusChange}
          onPointerDown={startDrag}
          onPointerMove={moveDrag}
          onPointerEnd={finishDrag}
        />
      ))}
      {dragPreview && <div className="board-drag-preview" style={{ left: dragPreview.x + 14, top: dragPreview.y + 14, width: dragPreview.width }}><strong>{dragPreview.title}</strong><span>{dragPreview.meta}</span></div>}
    </div>
  );
}

function TaskDetail({ task, project, tasks, closing, onClose, onEdit, onToggle, onTogglePin, onSubtask, onAcceptanceCriterion, onArchive, onDelete, onAddFile, onRemoveFile }: {
  task: Task; project: Project; tasks: Task[]; closing: boolean; onClose: () => void; onEdit: () => void; onToggle: () => void; onTogglePin: () => void; onSubtask: (id: string) => void; onAcceptanceCriterion: (id: string) => void; onArchive: () => void; onDelete: () => void; onAddFile: (kind: "attachments" | "images", file: Task["attachments"][number]) => void; onRemoveFile: (kind: "attachments" | "images", fileId: string) => void;
}) {
  const completedSubtasks = task.subtasks.filter((item) => item.completed).length;
  const [activityExpanded, setActivityExpanded] = useState(false);
  const [detailExpanded, setDetailExpanded] = useState(false);
  const scrollbar = useAutoHideScrollbar<HTMLElement>();
  const visibleActivity = activityItemsForDetail(task.activity, activityExpanded);
  useEffect(() => setActivityExpanded(false), [task.id]);
  return (
    <aside className={`detail-panel auto-hide-scrollbar ${detailExpanded ? "is-expanded" : ""} ${closing ? "is-closing" : ""}`} {...scrollbar}>
      <div className="detail-breadcrumb">{project.name}<span>›</span>{task.title}</div>
      <div className="detail-title-row"><h2>{task.title}</h2><div className="detail-title-actions"><button type="button" className={`icon-button detail-expand-button ${detailExpanded ? "is-expanded" : ""}`} onClick={() => setDetailExpanded((current) => !current)} aria-label={detailExpanded ? "收起任务详情" : "展开任务详情"} aria-pressed={detailExpanded} title={detailExpanded ? "收起任务详情" : "展开任务详情"}>{detailExpanded ? <ArrowsInSimple /> : <ArrowsOutSimple />}</button><button className={`icon-button detail-pin-button ${task.pinned ? "is-pinned" : ""}`} onClick={onTogglePin} aria-label={task.pinned ? "取消桌面置顶" : "置顶到桌面"} aria-pressed={task.pinned} title={task.pinned ? "取消桌面置顶" : "置顶到桌面"}><PushPin weight={task.pinned ? "fill" : "regular"} /></button><button className="icon-button" onClick={onEdit} aria-label="编辑任务" title="编辑任务"><PencilSimple /></button><button className="icon-button" onClick={onClose} aria-label="关闭详情"><X /></button></div></div>
      <dl className="task-meta">
        <div><dt>状态</dt><dd><span className={`state-dot ${task.status}`} />{statusLabel[task.status]}</dd></div>
        <div><dt>优先级</dt><dd>{task.priority === "high" && <ArrowUp className="priority-arrow" />} {task.priority === "high" ? "高" : task.priority === "medium" ? "中" : "低"}</dd></div>
        <div><dt>项目</dt><dd><span className="project-chip"><i className="project-color-dot" style={{ backgroundColor: project.color }} />{project.name}</span></dd></div>
        <div><dt>来源</dt><dd>{task.source}</dd></div>
        <div><dt>标签</dt><dd className="tags">{task.tags.map((tag) => <span key={tag}>{tag}</span>)}</dd></div>
      </dl>
      <section className="detail-section description-section">
        <h3>描述</h3>
        {task.description ? <p className="task-description">{task.description}</p> : <p className="empty-detail">暂无描述</p>}
      </section>
      <TaskFileSections taskId={task.id} attachments={task.attachments} images={task.images} onAdd={onAddFile} onRemove={onRemoveFile} />
      <section className="detail-section">
        <h3>子任务 <span>{completedSubtasks}/{task.subtasks.length}</span></h3>
        {task.subtasks.length ? task.subtasks.map((subtask) => (
          <label className="check-line" key={subtask.id}><input type="checkbox" checked={subtask.completed} onChange={() => onSubtask(subtask.id)} /><span>{subtask.title}</span><small>{subtask.completed ? "已完成" : "待完成"}</small></label>
        )) : <p className="empty-detail">暂无子任务</p>}
      </section>
      <section className="detail-section acceptance">
        <h3>验收标准</h3>
        {task.acceptanceCriteria.length ? task.acceptanceCriteria.map((criterion) => <label className="check-line" key={criterion.id}><input type="checkbox" checked={criterion.completed} onChange={() => onAcceptanceCriterion(criterion.id)} /><span>{criterion.title}</span></label>) : <p className="empty-detail">尚未设置验收标准</p>}
      </section>
      <section className="detail-section">
        <h3>依赖关系</h3>
        {task.dependencies.length ? task.dependencies.map((dependency) => {
          const dependencyTask = tasks.find((item) => item.title === dependency);
          const completed = dependencyTask?.status === "done";
          return <div className={`dependency ${completed ? "completed" : "pending"}`} key={dependency}>{completed ? <CheckCircle weight="fill" /> : <Circle />}<span>{dependency}</span><small>{dependencyTask ? (completed ? "已完成" : "未完成") : "外部依赖"}</small></div>;
        }) : <p className="empty-detail">没有阻塞依赖</p>}
      </section>
      <section className="detail-section activity">
        <header className="activity-heading">
          <h3>活动历史 <span>{task.activity.length}</span></h3>
          {task.activity.length > 4 && <button type="button" onClick={() => setActivityExpanded((current) => !current)}>{activityExpanded ? "收起" : `查看全部（${task.activity.length}）`}</button>}
        </header>
        <div className="activity-list">
          {visibleActivity.length ? visibleActivity.map((activity) => <div className="activity-item" key={activity.id}><LinkSimple /><span>{activity.action}</span><time dateTime={activity.at}>{formatActivityTime(activity.at)}</time></div>) : <p className="empty-detail">暂无活动记录</p>}
        </div>
      </section>
      <div className="detail-actions">
        {task.archived ? <><button className="secondary-action" onClick={onArchive}><ArrowCounterClockwise />恢复任务</button><button className="danger-action" onClick={onDelete}><Trash />永久删除</button></> : <><button className="secondary-action" onClick={onArchive}><Archive />归档</button><button className="secondary-action" onClick={onToggle}>{task.status === "done" ? "重新打开" : "标记完成"}</button></>}
      </div>
    </aside>
  );
}

function IntegrationView({ onStatusChange }: { onStatusChange: (status: CodexIntegrationStatus) => void }) {
  return (
    <div className="settings-page">
      <div className="settings-icon"><LinkSimple /></div><h1>连接与权限</h1>
      <p>TodoList 本体始终独立工作；连接后 Codex 才能按需读取、创建和更新任务。</p>
      <div className="settings-card"><div><strong>本地 MCP 程序</strong><span>STDIO sidecar 已随 TodoList 安装，桌面应用无需一直打开</span></div><span className="quiet-badge">已就绪</span></div>
      <CodexIntegrationCard onStatusChange={onStatusChange} />
      <div className="settings-card"><div><strong>当前数据范围</strong><span>启用后可读取、创建和更新全部本地项目</span></div><span className="quiet-badge">MVP</span></div>
      <div className="permission-note"><LockKey />写入前会检查任务版本；用户完成的任务不会被 Codex 默认重新打开。</div>
    </div>
  );
}

function SettingsView({ workspace, onImport }: { workspace: Workspace; onImport: (backup: WorkspaceBackup) => Promise<void> }) {
  return <div className="settings-page"><div className="settings-icon"><Gear /></div><h1>设置</h1><p>桌面便签默认始终置顶，但只会在你主动置顶任务后出现。</p><div className="settings-card"><div><strong>便签窗口</strong><span>不由 Codex 自动打开</span></div><span className="quiet-badge">推荐</span></div><DataBackupCard workspace={workspace} onImport={onImport} /><SoftwareUpdateCard /></div>;
}

function CreateTaskDialog({ projects, defaultProjectId, onClose, onCreate }: {
  projects: Project[]; defaultProjectId: string; onClose: () => void; onCreate: (title: string, projectId: string) => void;
}) {
  const [title, setTitle] = useState("");
  const [projectId, setProjectId] = useState(defaultProjectId);
  const submit = (event: FormEvent) => { event.preventDefault(); if (title.trim()) onCreate(title.trim(), projectId); };
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <form className="create-dialog" onSubmit={submit}>
        <div className="dialog-heading"><h2>新建任务</h2><button type="button" className="icon-button" onClick={onClose}><X /></button></div>
        <label>任务标题<input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} placeholder="需要完成什么？" /></label>
        <label>所属项目<select value={projectId} onChange={(event) => setProjectId(event.target.value)}>{projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}</select></label>
        <div className="dialog-actions"><button type="button" onClick={onClose}>取消</button><button className="dialog-primary" type="submit">创建任务</button></div>
      </form>
    </div>
  );
}

function MainApp() {
  const { workspace, ready, commit, storageState, storageMessage } = useWorkspace(true);
  const softwareUpdate = useSoftwareUpdate();
  const currentDate = useCurrentDate();
  const today = localIsoDate(currentDate);
  const [view, setView] = useState<View>({ kind: "today" });
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedTaskIds, setSelectedTaskIds] = useState<Set<string>>(() => new Set());
  const [detailClosing, setDetailClosing] = useState(false);
  const detailCloseTimer = useRef<number | null>(null);
  const [display, setDisplay] = useState<"list" | "board">("list");
  const [showCreate, setShowCreate] = useState(false);
  const [showCreateProject, setShowCreateProject] = useState(false);
  const [createTaskAfterProject, setCreateTaskAfterProject] = useState(false);
  const [editingTaskId, setEditingTaskId] = useState<string | null>(null);
  const [editingProjectId, setEditingProjectId] = useState<string | null>(null);
  const [pendingDeletion, setPendingDeletion] = useState<PendingDeletion>(null);
  const [completionNoticeTaskId, setCompletionNoticeTaskId] = useState<string | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [openingSticky, setOpeningSticky] = useState(false);
  const [windowError, setWindowError] = useState<string | null>(null);
  const [integrationStatus, setIntegrationStatus] = useState<CodexIntegrationStatus | null>(null);
  const [integrationError, setIntegrationError] = useState("");
  const openingStickyRef = useRef(false);
  const workspaceScrollbar = useAutoHideScrollbar<HTMLDivElement>();
  const acceptIntegrationStatus = useCallback((status: CodexIntegrationStatus) => {
    setIntegrationStatus(status);
    setIntegrationError("");
  }, []);
  const showSticky = async (minimizeMain: boolean) => {
    if (openingStickyRef.current) return;
    openingStickyRef.current = true;
    setOpeningSticky(true);
    setWindowError(null);
    try {
      await openStickyWindow(minimizeMain);
    } catch (error) {
      setWindowError(`打开便签失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      openingStickyRef.current = false;
      setOpeningSticky(false);
    }
  };

  useEffect(() => () => {
    if (detailCloseTimer.current !== null) window.clearTimeout(detailCloseTimer.current);
  }, []);

  useEffect(() => {
    void readCodexIntegrationStatus()
      .then(acceptIntegrationStatus)
      .catch((error) => setIntegrationError(error instanceof Error ? error.message : String(error)));
  }, [acceptIntegrationStatus]);

  const selectTask = (taskId: string) => {
    if (detailCloseTimer.current !== null) window.clearTimeout(detailCloseTimer.current);
    detailCloseTimer.current = null;
    setDetailClosing(false);
    setSelectedTaskId(taskId);
  };
  const clearSelectedTask = () => {
    if (detailCloseTimer.current !== null) window.clearTimeout(detailCloseTimer.current);
    detailCloseTimer.current = null;
    setDetailClosing(false);
    setSelectedTaskId(null);
  };
  const closeDetail = () => {
    if (!selectedTaskId || detailClosing) return;
    setDetailClosing(true);
    detailCloseTimer.current = window.setTimeout(() => {
      detailCloseTimer.current = null;
      setSelectedTaskId(null);
      setDetailClosing(false);
    }, DETAIL_TRANSITION_MS);
  };

  const scopedTasks = useMemo(() => {
    if (view.kind === "integration" || view.kind === "settings") return [];
    return tasksForView(workspace.tasks, view, today);
  }, [today, view, workspace.tasks]);
  const visibleTasks = useMemo(
    () => searchTasks(scopedTasks, workspace.projects, searchQuery),
    [scopedTasks, searchQuery, workspace.projects],
  );
  const visibleTaskKey = visibleTasks.map((task) => task.id).join("\u0000");
  useEffect(() => {
    setSelectedTaskIds((current) => reconcileSelectedTaskIds(current, visibleTasks.map((task) => task.id)));
  }, [visibleTaskKey]);

  const reorderVisibleTasks = (projectId: string, archived: boolean, visibleTaskIds: string[], orderedTaskIds: string[]) => {
    if (view.kind === "integration" || view.kind === "settings") return;
    const viewAtAction = view;
    const searchAtAction = searchQuery;
    const todayAtAction = today;
    commit((current) => {
      const currentVisibleTaskIds = searchTasks(tasksForView(current.tasks, viewAtAction, todayAtAction), current.projects, searchAtAction)
        .filter((task) => task.projectId === projectId && task.archived === archived)
        .map((task) => task.id);
      const requestedIds = new Set(visibleTaskIds);
      if (currentVisibleTaskIds.length !== requestedIds.size || currentVisibleTaskIds.some((taskId) => !requestedIds.has(taskId))) {
        throw new Error("当前任务结果已变化，未保存这次排序");
      }
      return reorderTaskSubset(current, projectId, archived, currentVisibleTaskIds, orderedTaskIds);
    });
  };

  const selectedTask = workspace.tasks.find((task) => task.id === selectedTaskId) ?? null;
  const selectedProject = selectedTask ? workspace.projects.find((project) => project.id === selectedTask.projectId) ?? null : null;
  const editingTask = workspace.tasks.find((task) => task.id === editingTaskId) ?? null;
  const editingProject = workspace.projects.find((project) => project.id === editingProjectId) ?? null;
  const deletingProject = pendingDeletion?.kind === "project" ? workspace.projects.find((project) => project.id === pendingDeletion.projectId) ?? null : null;
  const updateTask = (taskId: string, action: string, patch: Partial<Task>) => commit((current) => nextWorkspace(current, current.tasks.map((task) => task.id === taskId ? taskWithUserActivity(task, action, patch) : task)));
  const toggleTask = (taskId: string) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    if (!task) return;
    if (task.status !== "done" && task.acceptanceCriteria.some((criterion) => !criterion.completed)) {
      selectTask(taskId);
      setCompletionNoticeTaskId(taskId);
      return;
    }
    updateTask(taskId, task.status === "done" ? "重新打开任务" : "完成任务", { status: task.status === "done" ? "todo" : "done" });
  };
  const togglePin = (taskId: string) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    if (!task) return;
    const pinned = !task.pinned;
    updateTask(taskId, pinned ? "置顶到桌面" : "取消桌面置顶", { pinned });
    if (pinned) void showSticky(false);
  };
  const toggleSubtask = (taskId: string, subtaskId: string) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    const subtask = task?.subtasks.find((item) => item.id === subtaskId);
    if (task && subtask) {
      const completed = !subtask.completed;
      commit((current) => nextWorkspace(current, current.tasks.map((item) => item.id === taskId
        ? taskWithUserActivity(item, "更新子任务", { subtasks: item.subtasks.map((currentSubtask) => currentSubtask.id === subtaskId ? { ...currentSubtask, completed } : currentSubtask) })
        : item)));
    }
  };
  const toggleAcceptanceCriterion = (taskId: string, criterionId: string) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    const criterion = task?.acceptanceCriteria.find((item) => item.id === criterionId);
    if (task && criterion) {
      const completed = !criterion.completed;
      commit((current) => nextWorkspace(current, current.tasks.map((item) => item.id === taskId
        ? taskWithUserActivity(item, !completed && item.status === "done" ? "取消验收并重新打开任务" : "更新验收标准", {
            acceptanceCriteria: item.acceptanceCriteria.map((currentCriterion) => currentCriterion.id === criterionId ? { ...currentCriterion, completed } : currentCriterion),
            ...(!completed && item.status === "done" ? { status: "todo" as TaskStatus } : {}),
          })
        : item)));
    }
  };
  const changeTaskStatus = (taskId: string, status: TaskStatus) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    if (!task || task.status === status) return;
    if (status === "done" && task.acceptanceCriteria.some((criterion) => !criterion.completed)) {
      selectTask(taskId);
      setCompletionNoticeTaskId(taskId);
      return;
    }
    updateTask(taskId, `状态更新为${statusLabel[status]}`, { status });
  };
  const toggleArchive = (taskId: string) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    if (!task) return;
    const archived = !task.archived;
    updateTask(taskId, archived ? "归档任务" : "恢复归档任务", { archived, pinned: archived ? false : task.pinned });
    clearSelectedTask();
  };
  const deleteTask = (taskId: string) => {
    commit((current) => deleteArchivedTasksById(current, [taskId]));
    clearSelectedTask();
    setSelectedTaskIds((current) => { const next = new Set(current); next.delete(taskId); return next; });
    setPendingDeletion(null);
  };
  const createTask = (title: string, projectId: string) => {
    const task: Task = {
      id: crypto.randomUUID(), projectId, title, description: "", status: "todo", priority: "medium", dueLabel: "今天", dueDate: today, tags: [], source: "手动创建", archived: false, pinned: false, version: 1, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [],
      activity: [{ id: crypto.randomUUID(), action: "创建任务", actor: "user", at: new Date().toISOString() }],
    };
    commit((current) => nextWorkspace(current, [...current.tasks, task]));
    selectTask(task.id); setShowCreate(false);
  };
  const createProject = (name: string, color: string) => {
    const project: Project = { id: crypto.randomUUID(), name, color };
    commit((current) => ({ ...current, version: current.version + 1, projects: [...current.projects, project] }));
    setView({ kind: "project", projectId: project.id });
    clearSelectedTask();
    setShowCreateProject(false);
    if (createTaskAfterProject) setShowCreate(true);
    setCreateTaskAfterProject(false);
  };
  const saveProject = (projectId: string, name: string, color: string) => {
    commit((current) => ({ ...current, version: current.version + 1, projects: current.projects.map((project) => project.id === projectId ? { ...project, name, color } : project) }));
    setEditingProjectId(null);
  };
  const finishProjectDeletion = (projectId: string) => {
    if (view.kind === "project" && view.projectId === projectId) setView({ kind: "today" });
    setEditingProjectId(null);
    setPendingDeletion(null);
    setSelectedTaskIds(new Set());
    clearSelectedTask();
  };
  const moveTasksAndDeleteProject = (projectId: string, targetProjectId: string) => {
    commit((current) => moveProjectTasks(current, projectId, targetProjectId));
    finishProjectDeletion(projectId);
  };
  const moveTasksToNewProjectAndDelete = (projectId: string, name: string, color: string) => {
    const target: Project = { id: crypto.randomUUID(), name, color };
    commit((current) => moveProjectTasksToNewProject(current, projectId, target));
    finishProjectDeletion(projectId);
  };
  const deleteProject = (projectId: string) => {
    commit((current) => deleteProjectWithTasks(current, projectId));
    finishProjectDeletion(projectId);
  };
  const saveTask = (taskId: string, edits: TaskEdits) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    if (!task) return;
    try {
      validateTaskCompletion(task, { ...task, ...edits });
    } catch {
      selectTask(taskId);
      setCompletionNoticeTaskId(taskId);
      return;
    }
    commit((current) => nextWorkspace(current, current.tasks.map((item) => item.id === taskId
      ? taskWithUserActivity(item, "编辑任务", checklistEditsOnLatest(item, edits)) : item)));
    setEditingTaskId(null);
  };
  const importWorkspace = async (backup: WorkspaceBackup) => {
    let imported = backup.workspace;
    if (isTauri() && backup.managedFiles.length) {
      const mappings = await invoke<{ originalStorageKey: string; newStorageKey: string }[]>("restore_managed_files", { files: backup.managedFiles });
      imported = remapManagedFileStorageKeys(imported, mappings);
    }
    commit((current) => importedWorkspaceForSave(imported, current), true);
    clearSelectedTask();
    setSelectedTaskIds(new Set());
    setSearchQuery("");
  };

  const toggleMultiSelection = (taskId: string) => setSelectedTaskIds((current) => {
    const next = new Set(current);
    if (next.has(taskId)) next.delete(taskId); else next.add(taskId);
    return next;
  });
  const selectAllVisible = (selected: boolean) => setSelectedTaskIds(selected ? new Set(visibleTasks.map((task) => task.id)) : new Set());
  const batchArchive = () => {
    const ids = [...selectedTaskIds];
    commit((current) => archiveTasksById(current, ids));
    setSelectedTaskIds(new Set());
    if (selectedTaskId && ids.includes(selectedTaskId)) clearSelectedTask();
  };
  const batchDelete = (taskIds: string[]) => {
    commit((current) => deleteArchivedTasksById(current, taskIds));
    setSelectedTaskIds(new Set());
    if (selectedTaskId && taskIds.includes(selectedTaskId)) clearSelectedTask();
    setPendingDeletion(null);
  };
  const addTaskFile = (taskId: string, kind: "attachments" | "images", file: Task["attachments"][number]) => {
    commit((current) => addManagedFile(current, taskId, kind, file));
  };
  const removeTaskFile = (taskId: string, kind: "attachments" | "images", fileId: string) => {
    commit((current) => removeManagedFile(current, taskId, kind, fileId));
  };

  const currentTitle = view.kind === "today" ? "今日" : view.kind === "all" ? "全部" : view.kind === "active" ? "进行中" : view.kind === "archived" ? "已归档" : view.kind === "project" ? workspace.projects.find((project) => project.id === view.projectId)?.name ?? "项目" : "";
  const showWorkspace = view.kind !== "integration" && view.kind !== "settings";
  const defaultTaskProjectId = resolveDefaultProjectId(workspace.projects, view.kind === "project" ? view.projectId : undefined);
  const beginTaskCreation = () => {
    if (defaultTaskProjectId) {
      setShowCreate(true);
      return;
    }
    setCreateTaskAfterProject(true);
    setShowCreateProject(true);
  };

  return (
    <div className={`app-shell ${selectedTask && selectedProject ? "has-detail" : ""}`}>
      <Sidebar projects={workspace.projects} view={view} currentVersion={softwareUpdate.currentVersion} updateAvailable={softwareUpdate.phase === "available"} storageState={storageState} storageMessage={storageMessage} integrationStatus={integrationStatus} integrationError={integrationError} openingSticky={openingSticky} onOpenSticky={() => void showSticky(true)} onView={(next) => { setView(next); setSelectedTaskIds(new Set()); if (next.kind === "integration" || next.kind === "settings") clearSelectedTask(); }} onCreate={beginTaskCreation} onCreateProject={() => { setCreateTaskAfterProject(false); setShowCreateProject(true); }} onEditProject={setEditingProjectId} />
      <main className="workspace-panel">
        {!ready && <div className="loading-bar" />}
        {showWorkspace ? <>
          <header className="workspace-header">
            <div><h1>{currentTitle}<span>{todayHeading(currentDate)}</span></h1><p>{view.kind === "today" ? "包含所有项目中今天、到期或逾期的任务" : view.kind === "all" ? "所有项目中的未归档任务，包括未安排日期的待办" : view.kind === "active" ? "所有项目中正在处理的任务" : view.kind === "archived" ? "已归档任务可恢复或永久删除" : "查看和安排这个项目的任务"}</p></div>
             <div className="workspace-tools">{searchOpen ? <div className="search-box"><MagnifyingGlass /><input autoFocus value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} onKeyDown={(event) => { if (event.key === "Escape") { setSearchOpen(false); setSearchQuery(""); } }} placeholder={`搜索${currentTitle || "当前视图"}`} aria-label="搜索当前视图" /><button onClick={() => { setSearchOpen(false); setSearchQuery(""); }} aria-label="关闭搜索"><X /></button></div> : <button className="search-trigger" aria-label="搜索当前视图" onClick={() => setSearchOpen(true)}><MagnifyingGlass /><span>搜索</span></button>}<div className="view-switch"><button className={display === "list" ? "active" : ""} onClick={() => setDisplay("list")}><List />列表</button><button className={display === "board" ? "active" : ""} onClick={() => { setDisplay("board"); setSelectedTaskIds(new Set()); }}><Columns />看板</button></div></div>
          </header>
           <div className={`workspace-content auto-hide-scrollbar ${display === "board" ? "board-workspace" : "list-workspace"}`} role="region" aria-label={`${currentTitle}${display === "board" ? "任务看板" : "任务列表"}`} tabIndex={0} {...workspaceScrollbar}>
             {display === "list" && selectedTaskIds.size > 0 && <div className="batch-action-bar"><strong>已选 {selectedTaskIds.size} 项</strong><span>仅包含当前视图与搜索结果</span><button type="button" onClick={() => setSelectedTaskIds(new Set())}>取消选择</button>{view.kind === "archived" ? <button type="button" className="danger-action" onClick={() => setPendingDeletion({ kind: "batch", taskIds: [...selectedTaskIds] })}><Trash />永久删除</button> : <button type="button" className="secondary-action" onClick={batchArchive}><Archive />批量归档</button>}</div>}
             {visibleTasks.length ? (display === "list" ? <ListView projects={workspace.projects} tasks={visibleTasks} selectedTaskId={selectedTaskId} selectedTaskIds={selectedTaskIds} onSelect={selectTask} onToggle={toggleTask} onTogglePin={togglePin} onMultiSelect={toggleMultiSelection} onSelectAll={selectAllVisible} onReorder={reorderVisibleTasks} /> : <BoardView projects={workspace.projects} tasks={visibleTasks} onSelect={selectTask} onToggle={toggleTask} onStatusChange={changeTaskStatus} />) : <div className="empty-workspace"><MagnifyingGlass /><strong>{searchQuery.trim() ? "没有匹配的任务" : view.kind === "archived" ? "还没有归档任务" : workspace.projects.length ? "这里还没有任务" : "从第一个项目开始"}</strong><span>{searchQuery.trim() ? "试试搜索其他关键词" : view.kind === "archived" ? "归档的任务会保留在这里" : workspace.projects.length ? "点击左侧“新建任务”开始记录" : "新建任务时会先引导创建项目，首次使用不会自动添加演示数据"}</span></div>}
          </div>
          <footer className="workspace-footer">共 {visibleTasks.length} 个任务（未完成 {visibleTasks.filter((task) => task.status !== "done").length} 个）</footer>
        </> : view.kind === "integration" ? <IntegrationView onStatusChange={acceptIntegrationStatus} /> : <SettingsView workspace={workspace} onImport={importWorkspace} />}
      </main>
      {selectedTask && selectedProject && <><button type="button" className="detail-backdrop" aria-label="关闭任务详情" onClick={closeDetail} /><TaskDetail task={selectedTask} project={selectedProject} tasks={workspace.tasks} closing={detailClosing} onClose={closeDetail} onEdit={() => setEditingTaskId(selectedTask.id)} onToggle={() => toggleTask(selectedTask.id)} onTogglePin={() => togglePin(selectedTask.id)} onSubtask={(id) => toggleSubtask(selectedTask.id, id)} onAcceptanceCriterion={(id) => toggleAcceptanceCriterion(selectedTask.id, id)} onArchive={() => toggleArchive(selectedTask.id)} onDelete={() => setPendingDeletion({ kind: "task", taskId: selectedTask.id })} onAddFile={(kind, file) => addTaskFile(selectedTask.id, kind, file)} onRemoveFile={(kind, fileId) => removeTaskFile(selectedTask.id, kind, fileId)} /></>}
      {showCreate && defaultTaskProjectId && <CreateTaskDialog projects={workspace.projects} defaultProjectId={defaultTaskProjectId} onClose={() => setShowCreate(false)} onCreate={createTask} />}
      {showCreateProject && <ProjectEditorDialog projects={workspace.projects} onClose={() => { setShowCreateProject(false); setCreateTaskAfterProject(false); }} onSave={createProject} />}
      {editingProject && <ProjectEditorDialog projects={workspace.projects} project={editingProject} activeTaskCount={workspace.tasks.filter((task) => task.projectId === editingProject.id && !task.archived).length} archivedTaskCount={workspace.tasks.filter((task) => task.projectId === editingProject.id && task.archived).length} onClose={() => setEditingProjectId(null)} onSave={(name, color) => saveProject(editingProject.id, name, color)} onRequestDelete={() => { setEditingProjectId(null); setPendingDeletion({ kind: "project", projectId: editingProject.id }); }} />}
      {editingTask && <TaskEditorDialog task={editingTask} projects={workspace.projects} onClose={() => setEditingTaskId(null)} onSave={(edits) => saveTask(editingTask.id, edits)} />}
      {pendingDeletion?.kind === "task" && <ConfirmDialog title="永久删除任务？" description="这会从本机任务库中永久删除该任务，无法从“已归档”恢复。" confirmLabel="永久删除" onClose={() => setPendingDeletion(null)} onConfirm={() => deleteTask(pendingDeletion.taskId)} />}
      {deletingProject && <ProjectDeletionDialog project={deletingProject} projects={workspace.projects} activeTaskCount={workspace.tasks.filter((task) => task.projectId === deletingProject.id && !task.archived).length} archivedTaskCount={workspace.tasks.filter((task) => task.projectId === deletingProject.id && task.archived).length} onClose={() => setPendingDeletion(null)} onMove={(targetId) => moveTasksAndDeleteProject(deletingProject.id, targetId)} onMoveToNew={(name, color) => moveTasksToNewProjectAndDelete(deletingProject.id, name, color)} onDelete={() => deleteProject(deletingProject.id)} />}
      {pendingDeletion?.kind === "batch" && <ConfirmDialog title={`永久删除 ${pendingDeletion.taskIds.length} 个已归档任务？`} description="只会删除当前已选且仍处于“已归档”的任务；此操作不可恢复，相关托管附件与图片也会清理。" confirmLabel={`永久删除 ${pendingDeletion.taskIds.length} 项`} onClose={() => setPendingDeletion(null)} onConfirm={() => batchDelete(pendingDeletion.taskIds)} />}
      {completionNoticeTaskId && <NoticeDialog title="先确认验收标准" description="这项任务仍有未确认的验收标准。请在右侧详情中逐项确认后再标记完成。" onClose={() => setCompletionNoticeTaskId(null)} />}
      {windowError && <NoticeDialog title="无法打开便签" description={windowError} onClose={() => setWindowError(null)} />}
    </div>
  );
}

function StickyApp() {
  const { workspace, commit, storageState, storageMessage } = useWorkspace(true);
  const currentDate = useCurrentDate();
  const [completionNoticeTaskId, setCompletionNoticeTaskId] = useState<string | null>(null);
  const [openingMain, setOpeningMain] = useState(false);
  const [windowError, setWindowError] = useState<string | null>(null);
  const openingMainRef = useRef(false);
  const openTaskBoard = async () => {
    if (openingMainRef.current) return;
    openingMainRef.current = true;
    setOpeningMain(true);
    setWindowError(null);
    try {
      await openMainWindow();
    } catch (error) {
      setWindowError(`打开任务台失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      openingMainRef.current = false;
      setOpeningMain(false);
    }
  };
  const pinnedTasks = workspace.tasks.filter((task) => task.pinned);
  const toggleTask = (taskId: string) => {
    const task = workspace.tasks.find((item) => item.id === taskId);
    if (!task) return;
    if (task.status !== "done" && task.acceptanceCriteria.some((criterion) => !criterion.completed)) {
      setCompletionNoticeTaskId(taskId);
      return;
    }
    const status = task.status === "done" ? "todo" : "done";
    const action = task.status === "done" ? "从便签重新打开" : "从便签完成任务";
    commit((current) => nextWorkspace(current, current.tasks.map((item) => item.id === taskId ? taskWithUserActivity(item, action, { status }) : item)));
  };
  const unpin = (taskId: string) => commit((current) => nextWorkspace(current, current.tasks.map((task) => task.id === taskId ? taskWithUserActivity(task, "取消桌面置顶", { pinned: false }) : task)));
  return (
    <div className="sticky-window">
      <header className="sticky-header" data-tauri-drag-region="deep">
        <div><PushPin weight="fill" /><span>桌面任务</span><small>{pinnedTasks.length}</small></div>
        <button data-tauri-drag-region="false" onClick={() => void closeCurrentWindow()} aria-label="关闭便签"><X /></button>
      </header>
      <div className="sticky-body">
        {pinnedTasks.length ? pinnedTasks.map((task) => (
          <article className={`sticky-task ${task.status === "done" ? "completed" : ""}`} key={task.id}>
            <StatusButton task={task} onToggle={() => toggleTask(task.id)} />
            <div><strong>{task.title}</strong><span>{workspace.projects.find((project) => project.id === task.projectId)?.name} · {taskDueLabel(task.dueDate, task.dueLabel, currentDate)}</span></div>
            <button className="sticky-unpin" onClick={() => unpin(task.id)} title="取消置顶"><PushPinSlash /></button>
          </article>
        )) : <div className="sticky-empty"><PushPin /><strong>还没有置顶任务</strong><span>在任务行点亮图钉即可置顶</span></div>}
      </div>
      <footer className="sticky-footer"><button disabled={openingMain} aria-busy={openingMain} onClick={() => void openTaskBoard()}>{openingMain ? "正在打开……" : "打开任务台"}</button><span>拖动顶部可移动</span></footer>
      {storageState === "error" && <p className="sticky-storage-error" role="alert">{storageMessage}</p>}
      {completionNoticeTaskId && <NoticeDialog title="先确认验收标准" description="请打开任务台，在任务详情中逐项确认验收标准后再完成。" onClose={() => setCompletionNoticeTaskId(null)} />}
      {windowError && <NoticeDialog title="无法打开任务台" description={windowError} onClose={() => setWindowError(null)} />}
    </div>
  );
}

export function App() {
  return new URLSearchParams(window.location.search).get("mode") === "sticky" ? <StickyApp /> : <SoftwareUpdateProvider><MainApp /></SoftwareUpdateProvider>;
}
