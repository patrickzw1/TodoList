import { CalendarBlank, CaretLeft, CaretRight, Plus, Trash, X } from "@phosphor-icons/react";
import { FormEvent, type ComponentProps, type KeyboardEvent, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Priority, Project, Task, TaskStatus } from "./types";
import { calendarMonthCells, isValidIsoDate, localIsoDate, normalizeOptionalDueDate, parseIsoDate, shiftIsoDate, shiftIsoMonth, UNSCHEDULED_DUE_DATE } from "./date-utils.ts";
import { isPresetProjectColor, normalizeProjectColor, PROJECT_COLORS, PROJECT_COLOR_PRESETS } from "./project-colors.ts";
import { useAutoHideScrollbar } from "./use-auto-hide-scrollbar";
import { checklistEditorEdit, type TaskChecklistEdits } from "./task-checklists";

export type TaskEdits = {
  title: string;
  description: string;
  projectId: string;
  status: TaskStatus;
  priority: Priority;
  dueDate: string;
  dueLabel: string;
  tags: string[];
  dependencies: string[];
} & TaskChecklistEdits;

function splitTags(value: string) {
  return [...new Set(value.split(/[,，\n]/).map((item) => item.trim()).filter(Boolean))];
}

function AutoHideTextarea({ className = "", ...props }: ComponentProps<"textarea">) {
  const scrollbar = useAutoHideScrollbar<HTMLTextAreaElement>();
  return <textarea className={`auto-hide-scrollbar ${className}`.trim()} {...props} {...scrollbar} />;
}

const MONTH_OPTIONS = Array.from({ length: 12 }, (_, index) => index + 1);

function DatePicker({ value, forceInvalid, onChange }: {
  value: string;
  forceInvalid: boolean;
  onChange: (value: string) => void;
}) {
  const today = localIsoDate();
  const initialDate = parseIsoDate(value) ?? parseIsoDate(today)!;
  const [open, setOpen] = useState(false);
  const [manualTouched, setManualTouched] = useState(false);
  const [viewYear, setViewYear] = useState(initialDate.year);
  const [viewMonth, setViewMonth] = useState(initialDate.month);
  const [focusedDate, setFocusedDate] = useState(value && isValidIsoDate(value) ? value : today);
  const [position, setPosition] = useState({ top: 12, left: 12, width: 304 });
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const { ref: popoverScrollbarRef, ...popoverScrollbar } = useAutoHideScrollbar<HTMLDivElement>();
  const dialogId = useId();
  const errorId = useId();
  const invalid = Boolean(value.trim()) && !isValidIsoDate(value);
  const showError = invalid && (manualTouched || forceInvalid);
  const cells = useMemo(() => calendarMonthCells(viewYear, viewMonth), [viewMonth, viewYear]);
  const selected = isValidIsoDate(value) ? value : "";
  const years = useMemo(() => {
    const start = Math.min(1900, viewYear);
    const end = Math.max(2100, viewYear);
    return Array.from({ length: end - start + 1 }, (_, index) => start + index);
  }, [viewYear]);

  const syncView = (dateValue: string) => {
    const parsed = parseIsoDate(dateValue) ?? parseIsoDate(today)!;
    setViewYear(parsed.year);
    setViewMonth(parsed.month);
    setFocusedDate(dateValue && isValidIsoDate(dateValue) ? dateValue : today);
  };

  const placePopover = () => {
    const anchor = rootRef.current?.getBoundingClientRect();
    if (!anchor) return;
    const viewportPadding = 12;
    const width = Math.min(304, window.innerWidth - viewportPadding * 2);
    const height = popoverRef.current?.offsetHeight ?? 354;
    const left = Math.min(Math.max(viewportPadding, anchor.right - width), window.innerWidth - width - viewportPadding);
    const belowTop = anchor.bottom + 7;
    const aboveTop = anchor.top - height - 7;
    const top = belowTop + height <= window.innerHeight - viewportPadding
      ? belowTop
      : Math.max(viewportPadding, Math.min(aboveTop, window.innerHeight - height - viewportPadding));
    setPosition({ top, left, width });
  };

  const openPicker = () => {
    syncView(value);
    setOpen(true);
  };

  const closePicker = (restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const selectDate = (date: string) => {
    onChange(date);
    setManualTouched(false);
    closePicker(true);
  };

  const focusCalendarDate = (date: string | null) => {
    if (!date) return;
    const parsed = parseIsoDate(date);
    if (!parsed) return;
    setViewYear(parsed.year);
    setViewMonth(parsed.month);
    setFocusedDate(date);
  };

  const handleDayKeyDown = (event: KeyboardEvent<HTMLButtonElement>, date: string) => {
    let next: string | null = null;
    if (event.key === "ArrowLeft" || event.key === "Left") next = shiftIsoDate(date, -1);
    if (event.key === "ArrowRight" || event.key === "Right") next = shiftIsoDate(date, 1);
    if (event.key === "ArrowUp" || event.key === "Up") next = shiftIsoDate(date, -7);
    if (event.key === "ArrowDown" || event.key === "Down") next = shiftIsoDate(date, 7);
    if (event.key === "Home") next = shiftIsoDate(date, -dateFromIso(date).weekday);
    if (event.key === "End") next = shiftIsoDate(date, 6 - dateFromIso(date).weekday);
    if (event.key === "PageUp") next = shiftIsoMonth(date, -1);
    if (event.key === "PageDown") next = shiftIsoMonth(date, 1);
    if (!next) return;
    event.preventDefault();
    popoverRef.current?.querySelector<HTMLButtonElement>(`[data-date="${next}"]`)?.focus();
    focusCalendarDate(next);
  };

  useEffect(() => {
    if (!open) return;
    const animationFrame = window.requestAnimationFrame(placePopover);
    const handlePositionChange = () => placePopover();
    window.addEventListener("resize", handlePositionChange);
    document.addEventListener("scroll", handlePositionChange, true);
    return () => {
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("resize", handlePositionChange);
      document.removeEventListener("scroll", handlePositionChange, true);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return;
    popoverRef.current?.querySelector<HTMLButtonElement>(`[data-date="${focusedDate}"]`)?.focus();
  }, [focusedDate, open, viewMonth, viewYear]);

  const moveMonth = (offset: number) => {
    const anchor = `${String(viewYear).padStart(4, "0")}-${String(viewMonth).padStart(2, "0")}-01`;
    focusCalendarDate(shiftIsoMonth(anchor, offset));
  };

  const setCalendarMonth = (year: number, month: number) => {
    const current = parseIsoDate(focusedDate);
    const candidate = `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(current?.day ?? 1).padStart(2, "0")}`;
    const safeDate = isValidIsoDate(candidate) ? candidate : `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-01`;
    focusCalendarDate(safeDate);
  };

  const popover = open ? <><div className="date-picker-dismiss-layer" aria-hidden="true" onPointerDown={() => closePicker()} /><div
    ref={(element) => { popoverRef.current = element; popoverScrollbarRef.current = element; }}
    id={dialogId}
    className="date-picker-popover auto-hide-scrollbar"
    role="dialog"
    aria-label="选择截止日期"
    style={position}
    {...popoverScrollbar}
    onKeyDown={(event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      closePicker(true);
    }}
  >
    <div className="date-picker-heading">
      <button type="button" onClick={() => moveMonth(-1)} aria-label="上个月"><CaretLeft aria-hidden="true" /></button>
      <div>
        <select aria-label="年份" value={viewYear} onChange={(event) => setCalendarMonth(Number(event.target.value), viewMonth)}>{years.map((year) => <option key={year} value={year}>{year}年</option>)}</select>
        <select aria-label="月份" value={viewMonth} onChange={(event) => setCalendarMonth(viewYear, Number(event.target.value))}>{MONTH_OPTIONS.map((month) => <option key={month} value={month}>{month}月</option>)}</select>
      </div>
      <button type="button" onClick={() => moveMonth(1)} aria-label="下个月"><CaretRight aria-hidden="true" /></button>
    </div>
    <div className="date-picker-weekdays" aria-hidden="true">{"日一二三四五六".split("").map((day) => <span key={day}>{day}</span>)}</div>
    <div className="date-picker-grid" role="grid" aria-label={`${viewYear}年${viewMonth}月`}>
      {cells.map((cell, index) => <button
        type="button"
        role="gridcell"
        key={cell.date ?? `outside-range-${index}`}
        data-date={cell.date ?? undefined}
        disabled={!cell.date}
        className={`${cell.inCurrentMonth ? "" : "outside-month"} ${cell.date === today ? "is-today" : ""} ${cell.date === selected ? "is-selected" : ""}`.trim()}
        tabIndex={cell.date === focusedDate ? 0 : -1}
        aria-label={cell.date ? calendarAriaLabel(cell.date) : "超出支持的日期范围"}
        aria-selected={cell.date === selected}
        onFocus={() => cell.date && setFocusedDate(cell.date)}
        onKeyDown={(event) => cell.date && handleDayKeyDown(event, cell.date)}
        onClick={() => cell.date && selectDate(cell.date)}
      >{cell.day}</button>)}
    </div>
    <div className="date-picker-actions">
      <button type="button" onClick={() => { onChange(""); setManualTouched(false); closePicker(true); }}>清除</button>
      <button type="button" onClick={() => selectDate(today)}>今天</button>
    </div>
  </div></> : null;

  return <div className="date-picker" ref={rootRef}>
    <div className="date-picker-input-row">
      <input
        id="task-due-date"
        type="text"
        inputMode="numeric"
        autoComplete="off"
        maxLength={10}
        placeholder="YYYY-MM-DD"
        value={value}
        aria-invalid={showError || undefined}
        aria-describedby={showError ? errorId : undefined}
        onBlur={() => setManualTouched(true)}
        onChange={(event) => onChange(event.target.value)}
      />
      <button ref={triggerRef} type="button" className="date-picker-trigger" aria-label="打开日期选择器" aria-haspopup="dialog" aria-expanded={open} aria-controls={dialogId} onClick={() => open ? closePicker() : openPicker()}><CalendarBlank aria-hidden="true" /></button>
    </div>
    {showError && <span className="date-picker-error" id={errorId} role="alert">请输入有效日期（YYYY-MM-DD）。</span>}
    {createPortal(popover, document.body)}
  </div>;
}

function dateFromIso(value: string) {
  const parsed = parseIsoDate(value)!;
  const date = new Date(0);
  date.setUTCFullYear(parsed.year, parsed.month - 1, parsed.day);
  return { ...parsed, weekday: date.getUTCDay() };
}

function calendarAriaLabel(value: string) {
  const parsed = parseIsoDate(value)!;
  return `${parsed.year}年${parsed.month}月${parsed.day}日`;
}

interface TaskEntryDraft {
  id: string;
  value: string;
  existing: boolean;
}

function taskEntryControlId(sectionId: string, itemId: string) {
  return `${sectionId}-${itemId}`;
}

function TaskEntryEditor({ sectionId, label, addLabel, emptyLabel, placeholder, helper, items, invalidIds, onChange, onResolveInvalid }: {
  sectionId: string;
  label: string;
  addLabel: string;
  emptyLabel: string;
  placeholder: string;
  helper?: string;
  items: TaskEntryDraft[];
  invalidIds: Set<string>;
  onChange: (items: TaskEntryDraft[]) => void;
  onResolveInvalid: (controlId: string) => void;
}) {
  const headingId = `${sectionId}-heading`;
  const addItem = () => {
    const id = crypto.randomUUID();
    onChange([...items, { id, value: "", existing: false }]);
    window.requestAnimationFrame(() => document.getElementById(taskEntryControlId(sectionId, id))?.focus());
  };
  const removeItem = (id: string) => {
    const controlId = taskEntryControlId(sectionId, id);
    onResolveInvalid(controlId);
    onChange(items.filter((item) => item.id !== id));
  };

  return <section className="task-entry-section" aria-labelledby={headingId}>
    <div className="task-entry-heading">
      <div><h3 id={headingId}>{label}</h3><small>{items.length} 项</small></div>
      <button type="button" className="task-entry-add" onClick={addItem}><Plus aria-hidden="true" />{addLabel}</button>
    </div>
    {items.length ? <div className="task-entry-list">
      {items.map((item, index) => {
        const controlId = taskEntryControlId(sectionId, item.id);
        const errorId = `${controlId}-error`;
        const invalid = invalidIds.has(controlId);
        return <div className={`task-entry-row${invalid ? " is-invalid" : ""}`} key={item.id}>
          <span className="task-entry-number" aria-hidden="true">{index + 1}</span>
          <div className="task-entry-field">
            <AutoHideTextarea
              id={controlId}
              rows={2}
              value={item.value}
              onChange={(event) => {
                onResolveInvalid(controlId);
                onChange(items.map((current) => current.id === item.id ? { ...current, value: event.target.value } : current));
              }}
              placeholder={placeholder}
              aria-label={`${label} ${index + 1}`}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? errorId : undefined}
            />
            {invalid && <p className="task-entry-error" id={errorId} role="alert">内容不能为空，请填写或明确删除这一项。</p>}
          </div>
          <button type="button" className="task-entry-delete" onClick={() => removeItem(item.id)} aria-label={`删除${label} ${index + 1}`} title={`删除${label} ${index + 1}`}><Trash aria-hidden="true" /></button>
        </div>;
      })}
    </div> : <div className="task-entry-empty"><span>{emptyLabel}</span><button type="button" onClick={addItem}><Plus aria-hidden="true" />添加第一项</button></div>}
    {helper && <p className="task-entry-helper">{helper}</p>}
  </section>;
}

export function TaskEditorDialog({ task, projects, onClose, onSave }: {
  task: Task; projects: Project[]; onClose: () => void; onSave: (edits: TaskEdits) => void;
}) {
  const [initialChecklists] = useState(() => ({
    subtasks: task.subtasks.map((item) => ({ ...item })),
    acceptanceCriteria: task.acceptanceCriteria.map((item) => ({ ...item })),
  }));
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [projectId, setProjectId] = useState(task.projectId);
  const [status, setStatus] = useState<TaskStatus>(task.status);
  const [priority, setPriority] = useState<Priority>(task.priority);
  const [dueDate, setDueDate] = useState(task.dueDate === UNSCHEDULED_DUE_DATE ? "" : task.dueDate);
  const [dueDateInvalid, setDueDateInvalid] = useState(false);
  const [tags, setTags] = useState(task.tags.join("，"));
  const [subtasks, setSubtasks] = useState<TaskEntryDraft[]>(() => task.subtasks.map((item) => ({ id: item.id, value: item.title, existing: true })));
  const [acceptanceCriteria, setAcceptanceCriteria] = useState<TaskEntryDraft[]>(() => task.acceptanceCriteria.map((item) => ({ id: item.id, value: item.title, existing: true })));
  const [dependencies, setDependencies] = useState<TaskEntryDraft[]>(() => task.dependencies.map((value) => ({ id: crypto.randomUUID(), value, existing: true })));
  const [invalidEntryIds, setInvalidEntryIds] = useState<Set<string>>(() => new Set());
  const scrollbar = useAutoHideScrollbar<HTMLDivElement>();

  const resolveInvalidEntry = (controlId: string) => setInvalidEntryIds((current) => {
    if (!current.has(controlId)) return current;
    const next = new Set(current);
    next.delete(controlId);
    return next;
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmedTitle = title.trim();
    if (!trimmedTitle) return;
    if (dueDate.trim() && !isValidIsoDate(dueDate)) {
      setDueDateInvalid(true);
      window.requestAnimationFrame(() => document.getElementById("task-due-date")?.focus());
      return;
    }
    const sections = [
      { id: "task-subtasks", items: subtasks, setItems: setSubtasks },
      { id: "task-acceptance", items: acceptanceCriteria, setItems: setAcceptanceCriteria },
      { id: "task-dependencies", items: dependencies, setItems: setDependencies },
    ];
    const invalidIds = new Set(sections.flatMap((section) => section.items
      .filter((item) => item.existing && !item.value.trim())
      .map((item) => taskEntryControlId(section.id, item.id))));
    for (const section of sections) section.setItems(section.items.filter((item) => item.existing || item.value.trim()));
    setInvalidEntryIds(invalidIds);
    if (invalidIds.size) {
      const [firstInvalid] = invalidIds;
      window.requestAnimationFrame(() => document.getElementById(firstInvalid)?.focus());
      return;
    }
    const normalizedSubtasks = subtasks.filter((item) => item.existing || item.value.trim());
    const normalizedAcceptance = acceptanceCriteria.filter((item) => item.existing || item.value.trim());
    const normalizedDependencies = dependencies.filter((item) => item.existing || item.value.trim());
    const subtaskEdits = checklistEditorEdit(initialChecklists.subtasks, normalizedSubtasks.map((item) => ({ id: item.id, title: item.value })));
    const acceptanceEdits = checklistEditorEdit(initialChecklists.acceptanceCriteria, normalizedAcceptance.map((item) => ({ id: item.id, title: item.value })));
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
      ...(subtaskEdits ? { subtasks: subtaskEdits } : {}),
      ...(acceptanceEdits ? { acceptanceCriteria: acceptanceEdits } : {}),
      dependencies: normalizedDependencies.map((item) => item.value.trim()),
    });
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <form className="create-dialog task-editor-dialog" onSubmit={submit}>
        <div className="dialog-heading"><h2>编辑任务</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭编辑"><X /></button></div>
        <div className="editor-scroll auto-hide-scrollbar" {...scrollbar}>
          <label>任务标题<input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} /></label>
          <label>描述<AutoHideTextarea rows={3} value={description} onChange={(event) => setDescription(event.target.value)} placeholder="补充任务背景或要求" /></label>
          <div className="editor-grid">
            <label>所属项目<select value={projectId} onChange={(event) => setProjectId(event.target.value)}>{projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}</select></label>
            <label>状态<select value={status} onChange={(event) => setStatus(event.target.value as TaskStatus)}><option value="todo">待开始</option><option value="in_progress">进行中</option><option value="blocked">已阻塞</option><option value="done">已完成</option></select></label>
            <label>优先级<select value={priority} onChange={(event) => setPriority(event.target.value as Priority)}><option value="low">低</option><option value="medium">中</option><option value="high">高</option></select></label>
            <div className="editor-field"><label htmlFor="task-due-date">截止日期</label><DatePicker value={dueDate} forceInvalid={dueDateInvalid} onChange={(nextValue) => { setDueDate(nextValue); if (!nextValue.trim() || isValidIsoDate(nextValue)) setDueDateInvalid(false); }} /></div>
          </div>
          <label>标签<input value={tags} onChange={(event) => setTags(event.target.value)} placeholder="使用逗号分隔" /></label>
          <div className="task-entry-sections">
            <TaskEntryEditor sectionId="task-subtasks" label="子任务" addLabel="添加子任务" emptyLabel="还没有子任务" placeholder="填写一项子任务；Enter 仅换行" items={subtasks} invalidIds={invalidEntryIds} onChange={setSubtasks} onResolveInvalid={resolveInvalidEntry} />
            <TaskEntryEditor sectionId="task-acceptance" label="验收标准" addLabel="添加验收标准" emptyLabel="尚未设置验收标准" placeholder="填写一项验收标准；Enter 仅换行" items={acceptanceCriteria} invalidIds={invalidEntryIds} onChange={setAcceptanceCriteria} onResolveInvalid={resolveInvalidEntry} />
            <TaskEntryEditor sectionId="task-dependencies" label="依赖关系" addLabel="添加依赖" emptyLabel="没有阻塞依赖" placeholder="填写一个依赖；Enter 仅换行" helper="与现有任务标题完全一致时显示其完成状态，否则作为外部依赖保存。" items={dependencies} invalidIds={invalidEntryIds} onChange={setDependencies} onResolveInvalid={resolveInvalidEntry} />
          </div>
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
  const scrollbar = useAutoHideScrollbar<HTMLFormElement>();
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
      <form className="create-dialog project-dialog auto-hide-scrollbar" onSubmit={submit} {...scrollbar}>
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
  const scrollbar = useAutoHideScrollbar<HTMLDivElement>();
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
    <div className="create-dialog project-delete-dialog auto-hide-scrollbar" role="dialog" aria-modal="true" aria-labelledby="project-delete-title" {...scrollbar}>
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
  const scrollbar = useAutoHideScrollbar<HTMLDivElement>();
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="create-dialog confirm-dialog auto-hide-scrollbar" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-description" {...scrollbar}>
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
  const scrollbar = useAutoHideScrollbar<HTMLDivElement>();
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="create-dialog confirm-dialog auto-hide-scrollbar" role="alertdialog" aria-modal="true" aria-labelledby="notice-title" aria-describedby="notice-description" {...scrollbar}>
        <div className="dialog-heading"><h2 id="notice-title">{title}</h2><button type="button" className="icon-button" onClick={onClose} aria-label="关闭提示"><X /></button></div>
        <p id="notice-description">{description}</p>
        <div className="dialog-actions"><button type="button" className="dialog-primary" onClick={onClose}>知道了</button></div>
      </div>
    </div>
  );
}
