import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { seedWorkspace } from "./seed";
import { boundedActivity } from "./task-activity";
import { validateWorkspaceCompletion } from "./task-validation";
import type { Task, Workspace } from "./types";

const STORAGE_KEY = "todolist-workspace-v1";
const CHANGE_EVENT = "todolist-workspace-changed";
const VERSION_POLL_MS = 2000;
const READ_TIMEOUT_MS = 10000;
const MAX_SAVE_ATTEMPTS = 3;

export const emptyWorkspace: Workspace = { version: 1, projects: [], tasks: [] };

export type StorageState = "loading" | "saved" | "saving" | "merged" | "error";
export type WorkspaceReadTrace = { command: string; status: "ok" | "error"; durationMs: number; version?: number; projects?: number; tasks?: number };
type FrontendEvent = "bridge_ready" | "initial_read_started" | "initial_read_timed_out" | "initial_read_failed" | "workspace_applied" | "external_change" | "poll_failed" | "poll_recovered" | "save_failed" | "save_recovered";

function recordFrontend(event: FrontendEvent, workspace?: Workspace, durationMs?: number) {
  if (!isTauriRuntime()) return;
  void invoke("record_frontend_diagnostic", { diagnostic: {
    event,
    ...(workspace ? { workspaceVersion: workspace.version, projects: workspace.projects.length, tasks: workspace.tasks.length } : {}),
    ...(durationMs === undefined ? {} : { durationMs }),
  } }).catch(() => { /* Logging must never block storage. */ });
}

type WorkspaceUpdater = (current: Workspace) => Workspace;

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

function isDesktopHost() {
  return isTauriRuntime() || window.location.hostname === "tauri.localhost" || window.location.protocol === "tauri:";
}

async function readFromDesktop<T>(command: "load_workspace" | "load_workspace_version"): Promise<T> {
  if (!isTauriRuntime()) throw new Error("桌面桥接不可用；无法读取 SQLite");
  let timer = 0;
  const started = Date.now();
  if (command === "load_workspace") console.info("TodoList storage read started", { command });
  try {
    const result = await Promise.race([
      invoke<T>(command),
      new Promise<never>((_, reject) => { timer = window.setTimeout(() => reject(new Error(`${command} 读取超时`)), READ_TIMEOUT_MS); }),
    ]);
    const workspace = command === "load_workspace" ? result as Workspace | null : null;
    if (command === "load_workspace") console.info("TodoList storage read completed", {
      command, durationMs: Date.now() - started,
      version: workspace?.version ?? null,
      projects: workspace?.projects.length, tasks: workspace?.tasks.length,
    });
    return result;
  } catch (error) {
    console.warn("TodoList storage read failed", { command, durationMs: Date.now() - started });
    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function isVersionConflict(error: unknown) {
  return errorText(error).includes("workspace version conflict");
}

function readBrowserWorkspace(): Workspace {
  let saved: string | null;
  try { saved = window.localStorage.getItem(STORAGE_KEY); } catch (error) {
    if (!isTauriRuntime()) throw error;
    return structuredClone(emptyWorkspace);
  }
  if (!saved) return structuredClone(browserInitialWorkspace());
  try { return normalizeWorkspace(JSON.parse(saved) as Workspace); } catch { return structuredClone(emptyWorkspace); }
}

function browserInitialWorkspace() {
  return new URLSearchParams(window.location.search).get("demo") === "1" ? seedWorkspace : emptyWorkspace;
}

function normalizeWorkspace(workspace: Workspace): Workspace {
  return {
    ...workspace,
    tasks: workspace.tasks.map((task) => ({
      ...task,
      archived: task.archived ?? false,
      activity: boundedActivity(task.activity ?? []),
      acceptanceCriteria: (task.acceptanceCriteria as unknown[]).map((criterion, index) => typeof criterion === "string"
        ? { id: `legacy-${task.id}-${index}`, title: criterion, completed: false }
        : criterion as Task["acceptanceCriteria"][number]),
      attachments: task.attachments ?? [],
      images: task.images ?? [],
    })),
  };
}

function cacheWorkspace(workspace: Workspace, announce = false) {
  try { window.localStorage.setItem(STORAGE_KEY, JSON.stringify(workspace)); } catch (error) {
    // SQLite has already succeeded on desktop; this cache is only a hint.
    if (!isTauriRuntime()) throw error;
  }
  if (announce) window.dispatchEvent(new CustomEvent(CHANGE_EVENT));
}

async function loadPersistedWorkspace(): Promise<Workspace> {
  if (!isDesktopHost()) return readBrowserWorkspace();

  const workspace = await readFromDesktop<Workspace | null>("load_workspace");
  if (workspace) return workspace;

  const initial = structuredClone(emptyWorkspace);
  let timer = 0;
  try {
    await Promise.race([
      invoke("save_workspace", { workspace: initial }),
      new Promise<never>((_, reject) => { timer = window.setTimeout(() => reject(new Error("初始化工作区超时")), READ_TIMEOUT_MS); }),
    ]);
    return initial;
  } catch (error) {
    if (!isVersionConflict(error)) throw error;
    const concurrentlyCreated = await readFromDesktop<Workspace | null>("load_workspace");
    if (concurrentlyCreated) return concurrentlyCreated;
    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}

async function saveWithRebase(initial: Workspace, updater: WorkspaceUpdater, restore = false) {
  let candidate = initial;
  let merged = false;

  for (let attempt = 0; attempt < MAX_SAVE_ATTEMPTS; attempt += 1) {
    try {
      if (restore) candidate = await invoke<Workspace>("restore_workspace", { workspace: candidate });
      else await invoke("save_workspace", { workspace: candidate });
      cacheWorkspace(candidate, true);
      return { workspace: candidate, merged };
    } catch (error) {
      if (!isVersionConflict(error) || attempt === MAX_SAVE_ATTEMPTS - 1) throw error;
      const latest = await invoke<Workspace | null>("load_workspace");
      if (!latest) throw new Error("TodoList workspace disappeared while resolving a conflict");
      const rebased = updater(latest);
      if (rebased === latest) return { workspace: latest, merged: true };
      candidate = rebased;
      if (!restore) validateWorkspaceCompletion(latest, candidate);
      merged = true;
    }
  }

  throw new Error("TodoList could not save the workspace");
}

export function useWorkspace(pollForExternalChanges = false) {
  const [workspace, setWorkspace] = useState<Workspace>(() => isDesktopHost() ? structuredClone(emptyWorkspace) : readBrowserWorkspace());
  const [ready, setReady] = useState(!isDesktopHost());
  const [loaded, setLoaded] = useState(!isDesktopHost());
  const [storageState, setStorageState] = useState<StorageState>(isDesktopHost() ? "loading" : "saved");
  const [storageMessage, setStorageMessage] = useState(isDesktopHost() ? "正在读取本地任务" : "本地已保存");
  const [readTrace, setReadTrace] = useState<WorkspaceReadTrace | null>(null);
  const workspaceRef = useRef(workspace);
  const loadedRef = useRef(!isDesktopHost());
  const mountedRef = useRef(true);
  const pendingWritesRef = useRef(0);
  const changeEpochRef = useRef(0);
  const pollInFlightRef = useRef(false);
  const mergeObservedRef = useRef(false);
  const pollErrorRef = useRef(false);
  const saveErrorRef = useRef(false);
  const writeQueueRef = useRef<Promise<Workspace | undefined>>(Promise.resolve(undefined));

  const applyWorkspace = useCallback((next: Workspace, cache = true) => {
    workspaceRef.current = next;
    if (cache) cacheWorkspace(next);
    if (mountedRef.current) setWorkspace(next);
  }, []);

  const refresh = useCallback(async () => {
    const epoch = changeEpochRef.current;
    const started = Date.now();
    if (!loadedRef.current && mountedRef.current) {
      recordFrontend("initial_read_started");
      setReady(false);
      setStorageState("loading");
      setStorageMessage("正在读取本地任务");
    }
    try {
      const latest = await loadPersistedWorkspace();
      if (epoch !== changeEpochRef.current || pendingWritesRef.current > 0) return;
      applyWorkspace(latest);
      recordFrontend("workspace_applied", latest, Date.now() - started);
      loadedRef.current = true;
      if (mountedRef.current) {
        setReadTrace({ command: "load_workspace", status: "ok", durationMs: Date.now() - started, version: latest.version, projects: latest.projects.length, tasks: latest.tasks.length });
        setLoaded(true);
        setStorageState("saved");
        setStorageMessage("本地已保存");
      }
    } catch (error) {
      recordFrontend(errorText(error).includes("超时") ? "initial_read_timed_out" : "initial_read_failed", undefined, Date.now() - started);
      if (mountedRef.current) {
        setReadTrace({ command: "load_workspace", status: "error", durationMs: Date.now() - started });
        setStorageState("error");
        setStorageMessage(`读取失败：${errorText(error)}`);
      }
    } finally {
      if (mountedRef.current) setReady(true);
    }
  }, [applyWorkspace]);

  const refreshIfChanged = useCallback(async () => {
    if (!isDesktopHost() || !loadedRef.current || pendingWritesRef.current > 0 || pollInFlightRef.current) return;
    pollInFlightRef.current = true;
    const epoch = changeEpochRef.current;
    const started = Date.now();
    let command = "load_workspace_version";
    try {
      const persistedVersion = await readFromDesktop<number | null>("load_workspace_version");
      if (persistedVersion === null) throw new Error("工作区版本不存在");
      if (persistedVersion === workspaceRef.current.version) {
        if (pollErrorRef.current && mountedRef.current) {
          pollErrorRef.current = false;
          recordFrontend("poll_recovered", workspaceRef.current, Date.now() - started);
          setReadTrace({ command, status: "ok", durationMs: Date.now() - started, version: persistedVersion });
          setStorageState("saved");
          setStorageMessage("本地已保存");
        }
        return;
      }
      command = "load_workspace";
      const latest = await readFromDesktop<Workspace | null>("load_workspace");
      if (!latest || latest.version < persistedVersion) throw new Error("工作区快照与版本不一致");
      if (epoch !== changeEpochRef.current || pendingWritesRef.current > 0) return;
      applyWorkspace(latest);
      recordFrontend("external_change", latest, Date.now() - started);
      if (pollErrorRef.current) recordFrontend("poll_recovered", latest, Date.now() - started);
      pollErrorRef.current = false;
      if (mountedRef.current) {
        setReadTrace({ command, status: "ok", durationMs: Date.now() - started, version: latest.version, projects: latest.projects.length, tasks: latest.tasks.length });
        setStorageState("saved");
        setStorageMessage("已同步外部修改");
      }
    } catch (error) {
      recordFrontend("poll_failed", undefined, Date.now() - started);
      pollErrorRef.current = true;
      if (mountedRef.current) {
        setReadTrace({ command, status: "error", durationMs: Date.now() - started });
        setStorageState("error");
        setStorageMessage(`同步检查失败：${errorText(error)}`);
      }
    } finally {
      pollInFlightRef.current = false;
    }
  }, [applyWorkspace]);

  useEffect(() => {
    mountedRef.current = true;
    recordFrontend("bridge_ready");
    void refresh();
    return () => { mountedRef.current = false; };
  }, [refresh]);

  useEffect(() => {
    const handleChange = () => {
      if (pendingWritesRef.current > 0) return;
      if (isDesktopHost()) { void refreshIfChanged(); return; }
      const cached = readBrowserWorkspace();
      if (cached.version >= workspaceRef.current.version) applyWorkspace(cached, false);
    };
    window.addEventListener("storage", handleChange);
    window.addEventListener(CHANGE_EVENT, handleChange);
    const timer = pollForExternalChanges && isDesktopHost()
      ? window.setInterval(() => void refreshIfChanged(), VERSION_POLL_MS)
      : undefined;
    return () => {
      window.removeEventListener("storage", handleChange);
      window.removeEventListener(CHANGE_EVENT, handleChange);
      if (timer) window.clearInterval(timer);
    };
  }, [applyWorkspace, pollForExternalChanges, refreshIfChanged]);

  const commit = useCallback((updater: WorkspaceUpdater, restore = false) => {
    if (!loadedRef.current) return;
    const previous = workspaceRef.current;
    let optimistic: Workspace;
    try {
      optimistic = updater(previous);
      if (optimistic === previous) return;
      if (!restore) validateWorkspaceCompletion(previous, optimistic);
    } catch (error) {
      recordFrontend("save_failed");
      saveErrorRef.current = true;
      setStorageState("error");
      setStorageMessage(`保存失败：${errorText(error)}`);
      return;
    }
    changeEpochRef.current += 1;
    pendingWritesRef.current += 1;
    workspaceRef.current = optimistic;
    setWorkspace(optimistic);
    setStorageState("saving");
    setStorageMessage("正在保存");

    if (!isDesktopHost()) {
      try {
        cacheWorkspace(optimistic, true);
        setStorageState("saved");
        setStorageMessage("本地已保存");
      } catch (error) {
        applyWorkspace(previous, false);
        setStorageState("error");
        setStorageMessage(`保存失败：${errorText(error)}`);
      } finally {
        pendingWritesRef.current -= 1;
      }
      return;
    }

    const queued = pendingWritesRef.current > 1;
    const operation = writeQueueRef.current.then(async (lastSaved) => {
      // A restore assigns authoritative task versions inside SQLite. Reapply a
      // queued action to that result, or reload if the preceding action failed.
      const latest = queued ? lastSaved ?? await loadPersistedWorkspace() : undefined;
      const candidate = latest ? updater(latest) : optimistic;
      if (latest && candidate === latest) return { workspace: latest, merged: false };
      if (!restore && latest) validateWorkspaceCompletion(latest, candidate);
      return saveWithRebase(candidate, updater, restore);
    });
    writeQueueRef.current = operation.then(({ workspace: persisted }) => persisted, () => undefined);
    void operation.then(({ workspace: persisted, merged }) => {
      pendingWritesRef.current -= 1;
      mergeObservedRef.current ||= merged;
      if (pendingWritesRef.current > 0 || !mountedRef.current) return;
      applyWorkspace(persisted);
      if (saveErrorRef.current) {
        recordFrontend("save_recovered", persisted);
        saveErrorRef.current = false;
      }
      if (mergeObservedRef.current) {
        setStorageState("merged");
        setStorageMessage("已合并外部修改");
      } else {
        setStorageState("saved");
        setStorageMessage("本地已保存");
      }
      mergeObservedRef.current = false;
    }).catch(async (error) => {
      recordFrontend("save_failed");
      saveErrorRef.current = true;
      pendingWritesRef.current -= 1;
      if (!mountedRef.current) return;
      if (pendingWritesRef.current === 0) {
        try {
          applyWorkspace(await loadPersistedWorkspace());
        } catch (reloadError) {
          console.error("Could not reload TodoList after a failed save", reloadError);
        }
      }
      setStorageState("error");
      setStorageMessage(`保存失败：${errorText(error)}`);
    });
  }, [applyWorkspace, refresh]);

  return { workspace, ready, loaded, commit, refresh, storageState, storageMessage, readTrace };
}
