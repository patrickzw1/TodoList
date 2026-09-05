import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { seedWorkspace } from "./seed";
import { boundedActivity } from "./task-activity";
import { validateWorkspaceCompletion } from "./task-validation";
import type { Task, Workspace } from "./types";

const STORAGE_KEY = "todolist-workspace-v1";
const CHANGE_EVENT = "todolist-workspace-changed";
const VERSION_POLL_MS = 2000;
const MAX_SAVE_ATTEMPTS = 3;

export type StorageState = "saved" | "saving" | "merged" | "error";

type WorkspaceUpdater = (current: Workspace) => Workspace;

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
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
    return structuredClone(seedWorkspace);
  }
  if (!saved) return structuredClone(seedWorkspace);
  try { return normalizeWorkspace(JSON.parse(saved) as Workspace); } catch { return structuredClone(seedWorkspace); }
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
  if (!isTauriRuntime()) return readBrowserWorkspace();

  const workspace = await invoke<Workspace | null>("load_workspace");
  if (workspace) return workspace;

  const initial = structuredClone(seedWorkspace);
  try {
    await invoke("save_workspace", { workspace: initial });
    return initial;
  } catch (error) {
    if (!isVersionConflict(error)) throw error;
    const concurrentlyCreated = await invoke<Workspace | null>("load_workspace");
    if (concurrentlyCreated) return concurrentlyCreated;
    throw error;
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
      candidate = updater(latest);
      if (!restore) validateWorkspaceCompletion(latest, candidate);
      merged = true;
    }
  }

  throw new Error("TodoList could not save the workspace");
}

export function useWorkspace(pollForExternalChanges = false) {
  const [workspace, setWorkspace] = useState<Workspace>(readBrowserWorkspace);
  const [ready, setReady] = useState(false);
  const [storageState, setStorageState] = useState<StorageState>("saved");
  const [storageMessage, setStorageMessage] = useState("本地已保存");
  const workspaceRef = useRef(workspace);
  const mountedRef = useRef(true);
  const pendingWritesRef = useRef(0);
  const changeEpochRef = useRef(0);
  const pollInFlightRef = useRef(false);
  const mergeObservedRef = useRef(false);
  const writeQueueRef = useRef<Promise<Workspace | undefined>>(Promise.resolve(undefined));

  const applyWorkspace = useCallback((next: Workspace, cache = true) => {
    workspaceRef.current = next;
    if (cache) cacheWorkspace(next);
    if (mountedRef.current) setWorkspace(next);
  }, []);

  const refresh = useCallback(async () => {
    const epoch = changeEpochRef.current;
    try {
      const latest = await loadPersistedWorkspace();
      if (epoch !== changeEpochRef.current || pendingWritesRef.current > 0) return;
      applyWorkspace(latest);
      if (mountedRef.current) {
        setStorageState("saved");
        setStorageMessage("本地已保存");
      }
    } catch (error) {
      if (mountedRef.current) {
        setStorageState("error");
        setStorageMessage(`读取失败：${errorText(error)}`);
      }
    } finally {
      if (mountedRef.current) setReady(true);
    }
  }, [applyWorkspace]);

  const refreshIfChanged = useCallback(async () => {
    if (!isTauriRuntime() || pendingWritesRef.current > 0 || pollInFlightRef.current) return;
    pollInFlightRef.current = true;
    const epoch = changeEpochRef.current;
    try {
      const persistedVersion = await invoke<number | null>("load_workspace_version");
      if (persistedVersion === null || persistedVersion === workspaceRef.current.version) return;
      const latest = await invoke<Workspace | null>("load_workspace");
      if (!latest || epoch !== changeEpochRef.current || pendingWritesRef.current > 0) return;
      applyWorkspace(latest);
      if (mountedRef.current) {
        setStorageState("saved");
        setStorageMessage("已同步外部修改");
      }
    } catch (error) {
      console.warn("Could not check for external TodoList changes", error);
    } finally {
      pollInFlightRef.current = false;
    }
  }, [applyWorkspace]);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => { mountedRef.current = false; };
  }, [refresh]);

  useEffect(() => {
    const handleChange = () => {
      if (pendingWritesRef.current > 0) return;
      if (isTauriRuntime()) { void refreshIfChanged(); return; }
      const cached = readBrowserWorkspace();
      if (cached.version >= workspaceRef.current.version) applyWorkspace(cached, false);
    };
    window.addEventListener("storage", handleChange);
    window.addEventListener(CHANGE_EVENT, handleChange);
    const timer = pollForExternalChanges && isTauriRuntime()
      ? window.setInterval(() => void refreshIfChanged(), VERSION_POLL_MS)
      : undefined;
    return () => {
      window.removeEventListener("storage", handleChange);
      window.removeEventListener(CHANGE_EVENT, handleChange);
      if (timer) window.clearInterval(timer);
    };
  }, [applyWorkspace, pollForExternalChanges, refreshIfChanged]);

  const commit = useCallback((updater: WorkspaceUpdater, restore = false) => {
    const previous = workspaceRef.current;
    let optimistic: Workspace;
    try {
      optimistic = updater(previous);
      if (!restore) validateWorkspaceCompletion(previous, optimistic);
    } catch (error) {
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

    if (!isTauriRuntime()) {
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
      if (!restore && latest) validateWorkspaceCompletion(latest, candidate);
      return saveWithRebase(candidate, updater, restore);
    });
    writeQueueRef.current = operation.then(({ workspace: persisted }) => persisted, () => undefined);
    void operation.then(({ workspace: persisted, merged }) => {
      pendingWritesRef.current -= 1;
      mergeObservedRef.current ||= merged;
      if (pendingWritesRef.current > 0 || !mountedRef.current) return;
      applyWorkspace(persisted);
      if (mergeObservedRef.current) {
        setStorageState("merged");
        setStorageMessage("已合并外部修改");
      } else {
        setStorageState("saved");
        setStorageMessage("本地已保存");
      }
      mergeObservedRef.current = false;
    }).catch(async (error) => {
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

  return { workspace, ready, commit, refresh, storageState, storageMessage };
}
