import { Plus, X } from "@phosphor-icons/react";
import { FormEvent, useEffect, useId, useMemo, useState } from "react";
import type { AcceptanceCriterion, Priority, Project, Subtask, Task, TaskStatus } from "./types";
import { normalizeOptionalDueDate, UNSCHEDULED_DUE_DATE } from "./date-utils.ts";
import { isPresetProjectColor, normalizeProjectColor, PROJECT_COLORS, PROJECT_COLOR_PRESETS } from "./project-colors.ts";
import { useAutoHideScrollbar } from "./use-auto-hide-scrollbar";
import { reconcileAcceptanceCriteria, reconcileSubtasks } from "./task-checklists";

export interface TaskEdits {
  title: string;
  description: string;
  projectId: string;
  status: TaskStatus;
  priority: Priority;
  dueDate: string;
  dueLabel: string;
  tags: string[];
  subtasks?: Subtask[];
  acceptanceCriteria?: AcceptanceCriterion[];
  dependencies: string[];
}

function splitLines(value: string) {
  return value.split(/\r?\n/).map((item) => item.trim()).filter(Boolean);
}

function splitTags(value: string) {
  return [...new Set(value.split(/[,，\n]/).map((item) => item.trim()).filter(Boolean))];
}

export function TaskEditorDialog({ task, projects, onClose, onSave }: {
  task: Task; projects: Project[]; onClose: () => void; onSave: (edits: TaskEdits) => void;
}) {
  const [initialChecklists] = useState(() => ({
    subtasks: task.subtasks.map((item) => item.title).join("\n"),
    acceptanceCriteria: task.acceptanceCriteria.map((item) => item.title).join("\n"),
  }));
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [projectId, setProjectId] = useState(task.projectId);
  const [status, setStatus] = useState<TaskStatus>(task.status);
  const [priority, setPriority] = useState<Priority>(task.priority);
  const [dueDate, setDueDate] = useState(task.dueDate === UNSCHEDULED_DUE_DATE ? "" : task.dueDate);
  const [tags, setTags] = useState(task.tags.join("，"));
  const [subtasks, setSubtasks] = useState(task.subtasks.map((item) => item.title).join("\n"));
  const [acceptanceCriteria, setAcceptanceCriteria] = useState(task.acceptanceCriteria.map((item) => item.title).join("\n"));
  const [dependencies, setDependencies] = useState(task.dependencies.join("\n"));
  const scrollbar = useAutoHideScrollbar<HTMLDivElement>();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmedTitle = title.trim();
    if (!trimmedTitle) return;
    const resolvedDueDate = normalizeOptionalDueDate(dueDate);
    onSave({
      title: trimmedTitle,
      description: description.trim(),
      projectId,
      status,
      priority,
      dueDate: resolvedDueDate,
      dueLabel: resolvedDueDate === UNSCHEDULED_DUE_DATE ? "未安排" : resolvedDueDate,
      tags: splitTags(tags),
      ...(subtasks !== initialChecklists.subtasks ? { subtasks: reconcileSubtasks(task.subtasks, splitLines(subtasks)) } : {}),
      ...(acceptanceCriteria !== initialChecklists.acceptanceCriteria ? { acceptanceCriteria: reconcileAcceptanceCriteria(task.acceptanceCriteria, splitLines(acceptanceCriteria)) } : {}),
      dependencies: splitLines(dependencies),
    });
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <form className="create-dialog task-editor-dialog" onSubmit={submit}>
        <div className="dialog-heading"><h2>编辑任务</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭编辑"><X /></button></div>
        <div className="editor-scroll auto-hide-scrollbar" {...scrollbar}>
          <label>任务标题<input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} /></label>
          <label>描述<textarea rows={3} value={description} onChange={(event) => setDescription(event.target.value)} placeholder="补充任务背景或要求" /></label>
          <div className="editor-grid">
            <label>所属项目<select value={projectId} onChange={(event) => setProjectId(event.target.value)}>{projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}</select></label>
            <label>状态<select value={status} onChange={(event) => setStatus(event.target.value as TaskStatus)}><option value="todo">待开始</option><option value="in_progress">进行中</option><option value="blocked">已阻塞</option><option value="done">已完成</option></select></label>
            <label>优先级<select value={priority} onChange={(event) => setPriority(event.target.value as Priority)}><option value="low">低</option><option value="medium">中</option><option value="high">高</option></select></label>
            <label>截止日期<input type="date" value={dueDate} onChange={(event) => setDueDate(event.target.value)} /></label>
          </div>
          <label>标签<input value={tags} onChange={(event) => setTags(event.target.value)} placeholder="使用逗号分隔" /></label>
          <div className="editor-grid text-lists">
            <label>子任务<textarea rows={5} value={subtasks} onChange={(event) => setSubtasks(event.target.value)} placeholder="每行一个子任务" /></label>
            <label>验收标准<textarea rows={5} value={acceptanceCriteria} onChange={(event) => setAcceptanceCriteria(event.target.value)} placeholder="每行一项验收标准" /></label>
          </div>
          <label>依赖关系<textarea rows={3} value={dependencies} onChange={(event) => setDependencies(event.target.value)} placeholder="每行一个依赖" /></label>
        </div>
        <div className="dialog-actions"><button type="button" onClick={onClose}>取消</button><button className="dialog-primary" type="submit" disabled={!title.trim()}>保存修改</button></div>
      </form>
    </div>
  );
}

function ProjectColorPicker({ value, onChange, onValidityChange }: {
  value: string;
  onChange: (color: string) => void;
  onValidityChange: (valid: boolean) => void;
}) {
  const normalizedValue = normalizeProjectColor(value) ?? PROJECT_COLORS[0];
  const [customExpanded, setCustomExpanded] = useState(!isPresetProjectColor(normalizedValue));
  const [hexInput, setHexInput] = useState(normalizedValue);
  const [error, setError] = useState("");
  const panelId = useId();
  const errorId = useId();
  const customSelected = !isPresetProjectColor(normalizedValue);

  useEffect(() => {
    onValidityChange(true);
  }, [onValidityChange]);

  const selectPreset = (color: string) => {
    setCustomExpanded(false);
    setHexInput(color);
    setError("");
    onValidityChange(true);
    onChange(color);
  };
  const applyCustomColor = (raw: string) => {
    setHexInput(raw);
    const normalized = normalizeProjectColor(raw);
    if (!normalized) {
      setError("请输入 #RGB 或 #RRGGBB 格式的颜色");
      onValidityChange(false);
      return;
    }
    setError("");
    onValidityChange(true);
    onChange(normalized);
  };

  return <fieldset className="color-picker">
    <legend>项目颜色</legend>
    <div className="color-picker-options">
      {PROJECT_COLOR_PRESETS.map(({ value: item, label }) => <button type="button" key={item} className={`color-preset${normalizedValue === item ? " selected" : ""}`} style={{ background: item }} onClick={() => selectPreset(item)} aria-label={`选择${label} ${item.toUpperCase()}`} title={`${label} ${item.toUpperCase()}`} aria-pressed={normalizedValue === item} />)}
      <button type="button" className={`custom-color-toggle${customSelected ? " selected" : ""}`} onClick={() => setCustomExpanded((current) => !current)} aria-expanded={customExpanded} aria-controls={panelId} aria-pressed={customSelected}>
        <span className="custom-color-swatch" style={{ background: normalizedValue }} aria-hidden="true" />
        自定义
      </button>
    </div>
    {customExpanded && <div className="custom-color-panel" id={panelId}>
      <label className="native-color-field">取色<input type="color" value={normalizedValue} onChange={(event) => applyCustomColor(event.target.value)} aria-label="自定义项目颜色取色器" /></label>
      <label className="hex-color-field">HEX<input value={hexInput} onChange={(event) => applyCustomColor(event.target.value)} onBlur={() => { const normalized = normalizeProjectColor(hexInput); if (normalized) setHexInput(normalized); }} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined} placeholder="#RRGGBB" spellCheck={false} /></label>
      <span className="custom-color-preview" style={{ background: normalizedValue }} title={`当前颜色 ${normalizedValue}`} aria-label={`当前颜色 ${normalizedValue}`} />
    </div>}
    {error && <p className="color-picker-error" id={errorId} role="alert">{error}</p>}
  </fieldset>;
}

export function ProjectEditorDialog({ projects, project, activeTaskCount = 0, archivedTaskCount = 0, onClose, onSave, onRequestDelete }: {
  projects: Project[]; project?: Project; activeTaskCount?: number; archivedTaskCount?: number; onClose: () => void; onSave: (name: string, color: string) => void; onRequestDelete?: () => void;
}) {
  const [name, setName] = useState(project?.name ?? "");
  const [color, setColor] = useState(normalizeProjectColor(project?.color ?? "") ?? PROJECT_COLORS[projects.length % PROJECT_COLORS.length]);
  const [colorValid, setColorValid] = useState(true);
  const duplicate = useMemo(() => projects.some((item) => item.id !== project?.id && item.name.trim().toLocaleLowerCase() === name.trim().toLocaleLowerCase()), [name, project?.id, projects]);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (name.trim() && !duplicate && colorValid) onSave(name.trim(), color);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <form className="create-dialog project-dialog" onSubmit={submit}>
        <div className="dialog-heading"><h2>{project ? "编辑项目" : "新建项目"}</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭项目编辑"><X /></button></div>
        <label>项目名称<input autoFocus value={name} onChange={(event) => setName(event.target.value)} placeholder="例如：个人网站发布" /></label>
        <ProjectColorPicker value={color} onChange={setColor} onValidityChange={setColorValid} />
        {duplicate && <p className="dialog-error">已经存在同名项目</p>}
        {project && (activeTaskCount > 0 || archivedTaskCount > 0) && <p className="dialog-note">项目包含 {activeTaskCount} 个活跃任务和 {archivedTaskCount} 个已归档任务。删除时可选择转移或一起永久删除。</p>}
        <div className="dialog-actions">{project && onRequestDelete && <button type="button" className="danger-action push-left" onClick={onRequestDelete}>删除项目</button>}<button type="button" onClick={onClose}>取消</button><button className="dialog-primary" type="submit" disabled={!name.trim() || duplicate || !colorValid}>{project ? "保存项目" : <><Plus />创建项目</>}</button></div>
      </form>
    </div>
  );
}

export function ProjectDeletionDialog({ project, projects, activeTaskCount, archivedTaskCount, onClose, onMove, onMoveToNew, onDelete }: {
  project: Project;
  projects: Project[];
  activeTaskCount: number;
  archivedTaskCount: number;
  onClose: () => void;
  onMove: (targetProjectId: string) => void;
  onMoveToNew: (name: string, color: string) => void;
  onDelete: () => void;
}) {
  const destinations = projects.filter((item) => item.id !== project.id);
  const [mode, setMode] = useState<"move" | "delete">(activeTaskCount + archivedTaskCount > 0 ? "move" : "delete");
  const [target, setTarget] = useState(destinations[0]?.id ?? "new");
  const [newName, setNewName] = useState("");
  const [newColor, setNewColor] = useState<string>(PROJECT_COLORS[projects.length % PROJECT_COLORS.length]);
  const [newColorValid, setNewColorValid] = useState(true);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const duplicate = projects.some((item) => item.id !== project.id && item.name.trim().toLocaleLowerCase() === newName.trim().toLocaleLowerCase());
  const total = activeTaskCount + archivedTaskCount;

  if (confirmDelete) {
    return <ConfirmDialog
      title="再次确认永久删除？"
      description={`项目“${project.name}”及其中 ${activeTaskCount} 个活跃任务、${archivedTaskCount} 个已归档任务将被永久删除，相关托管附件和图片也会清理。此操作不可恢复。`}
      confirmLabel="永久删除项目及任务"
      onClose={() => setConfirmDelete(false)}
      onConfirm={onDelete}
    />;
  }

  const confirm = () => {
    if (mode === "delete") { setConfirmDelete(true); return; }
    if (target === "new") {
      if (newName.trim() && !duplicate && newColorValid) onMoveToNew(newName.trim(), newColor);
      return;
    }
    onMove(target);
  };

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
    <div className="create-dialog project-delete-dialog" role="dialog" aria-modal="true" aria-labelledby="project-delete-title">
      <div className="dialog-heading"><h2 id="project-delete-title">删除项目“{project.name}”</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭项目删除"><X /></button></div>
      <p className="project-delete-summary">当前有 <strong>{activeTaskCount}</strong> 个活跃任务、<strong>{archivedTaskCount}</strong> 个已归档任务。</p>
      <label className="project-delete-option"><input type="radio" name="delete-mode" checked={mode === "move"} onChange={() => setMode("move")} /><span><strong>保留任务并转移</strong><small>任务、归档状态、附件与图片都会保留。</small></span></label>
      {mode === "move" && <div className="project-delete-target">
        <label>承接项目<select value={target} onChange={(event) => setTarget(event.target.value)}>{destinations.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}<option value="new">新建承接项目…</option></select></label>
        {target === "new" && <><label>新项目名称<input autoFocus value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="例如：未分类任务" /></label><ProjectColorPicker value={newColor} onChange={setNewColor} onValidityChange={setNewColorValid} />{duplicate && <p className="dialog-error">已经存在同名项目</p>}</>}
      </div>}
      <label className="project-delete-option is-danger"><input type="radio" name="delete-mode" checked={mode === "delete"} onChange={() => setMode("delete")} /><span><strong>项目和任务一起永久删除</strong><small>{total ? `将删除全部 ${total} 个任务，下一步仍需再次确认。` : "空项目仍需下一步确认。"}</small></span></label>
      <div className="dialog-actions"><button type="button" onClick={onClose}>取消</button><button type="button" className={mode === "delete" ? "danger-action" : "dialog-primary"} disabled={mode === "move" && target === "new" && (!newName.trim() || duplicate || !newColorValid)} onClick={confirm}>{mode === "delete" ? "继续永久删除" : "转移并删除项目"}</button></div>
    </div>
  </div>;
}

export function ConfirmDialog({ title, description, confirmLabel, onClose, onConfirm }: {
  title: string; description: string; confirmLabel: string; onClose: () => void; onConfirm: () => void;
}) {
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="create-dialog confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-description">
        <div className="dialog-heading"><h2 id="confirm-title">{title}</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭确认"><X /></button></div>
        <p id="confirm-description">{description}</p>
        <div className="dialog-actions"><button type="button" onClick={onClose}>取消</button><button type="button" className="danger-action" onClick={onConfirm}>{confirmLabel}</button></div>
      </div>
    </div>
  );
}

export function NoticeDialog({ title, description, onClose }: {
  title: string; description: string; onClose: () => void;
}) {
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="create-dialog confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="notice-title" aria-describedby="notice-description">
        <div className="dialog-heading"><h2 id="notice-title">{title}</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭提示"><X /></button></div>
        <p id="notice-description">{description}</p>
        <div className="dialog-actions"><button type="button" className="dialog-primary" onClick={onClose}>知道了</button></div>
      </div>
    </div>
  );
}
