import { friendlyUpdateError, type UpdateFailureStage } from "./update-status.ts";

export type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "downloaded" | "installing" | "latest" | "error";
export type ComponentBuildStatus = { desktopBuild: string; mcpBuild?: string; matches: boolean; message: string };
export type UpdateRecovery = { attemptId: string; version: string; state: "failed" | "cancelled"; reason: string; installerDirectory: string; explorerOpened: boolean };
export type UpdateDownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished"; data?: Record<string, never> };

export interface ManagedSoftwareUpdate {
  version: string;
  download(onEvent: (event: UpdateDownloadEvent) => void, options: { timeout: number }): Promise<void>;
  install(options: { restartAfterInstall: boolean }): Promise<void>;
  close(): Promise<void>;
}

export interface SoftwareUpdateRuntime {
  desktop: boolean;
  buildVersion: string;
  now?: () => number;
  getCurrentVersion?: () => Promise<string>;
  check?: () => Promise<ManagedSoftwareUpdate | null>;
  getComponentStatus?: () => Promise<ComponentBuildStatus | null>;
  getRecovery?: () => Promise<UpdateRecovery | null>;
  retryRecovery?: (attemptId: string) => Promise<void>;
  discardRecovery?: (attemptId: string) => Promise<void>;
  openRecoveryLocation?: (attemptId: string) => Promise<void>;
  relaunch?: () => Promise<void>;
}

export interface SoftwareUpdateSnapshot {
  desktop: boolean;
  currentVersion: string;
  availableVersion: string | null;
  phase: UpdatePhase;
  message: string;
  progress: number | null;
  componentStatus: ComponentBuildStatus | null;
  recovery: UpdateRecovery | null;
}

export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export function createSoftwareUpdateManager(runtime: SoftwareUpdateRuntime) {
  const listeners = new Set<(snapshot: SoftwareUpdateSnapshot) => void>();
  const now = runtime.now ?? Date.now;
  let disposed = false;
  let initialized = false;
  let lastCheckAttempt = 0;
  let checkInFlight: Promise<void> | null = null;
  let availableUpdate: ManagedSoftwareUpdate | null = null;
  let snapshot: SoftwareUpdateSnapshot = {
    desktop: runtime.desktop,
    currentVersion: runtime.buildVersion,
    availableVersion: null,
    phase: "idle",
    message: runtime.desktop ? "TodoList 会在后台定期检查更新，也可随时手动检查。" : "网页预览不访问桌面更新源。",
    progress: null,
    componentStatus: null,
    recovery: null,
  };

  const publish = (patch: Partial<SoftwareUpdateSnapshot>) => {
    if (disposed) return;
    snapshot = { ...snapshot, ...patch };
    for (const listener of listeners) listener(snapshot);
  };

  const closeAvailableUpdate = async () => {
    const previous = availableUpdate;
    availableUpdate = null;
    if (previous) await previous.close();
  };

  const showFailure = (error: unknown, stage: UpdateFailureStage) => {
    publish({ phase: "error", message: friendlyUpdateError(error, stage), progress: null, availableVersion: null });
  };

  const checkForUpdate = () => {
    if (!runtime.desktop || disposed || !runtime.check) return Promise.resolve();
    if (checkInFlight) return checkInFlight;
    if (["downloading", "downloaded", "installing"].includes(snapshot.phase)) return Promise.resolve();
    lastCheckAttempt = now();
    const operation = (async () => {
      publish({ phase: "checking", message: "正在安全检查更新……", progress: null, availableVersion: null });
      try {
        await closeAvailableUpdate();
        const update = await runtime.check!();
        if (disposed) {
          if (update) await update.close();
          return;
        }
        availableUpdate = update;
        if (update) {
          publish({ phase: "available", availableVersion: update.version, message: `发现新版本 ${update.version}，下载前不会启动安装。` });
        } else {
          publish({ phase: "latest", availableVersion: null, message: "当前已经是最新版本。" });
        }
      } catch (error) {
        showFailure(error, "check");
      }
    })();
    checkInFlight = operation.finally(() => { checkInFlight = null; });
    return checkInFlight;
  };

  const checkIfStale = () => {
    if (!runtime.desktop || disposed || (lastCheckAttempt > 0 && now() - lastCheckAttempt < UPDATE_CHECK_INTERVAL_MS)) {
      return Promise.resolve();
    }
    return checkForUpdate();
  };

  const initialize = async () => {
    if (initialized || disposed) return;
    initialized = true;
    if (!runtime.desktop) return;
    const [version, componentStatus, recovery] = await Promise.all([
      runtime.getCurrentVersion?.().catch(() => runtime.buildVersion) ?? runtime.buildVersion,
      runtime.getComponentStatus?.().catch(() => null) ?? null,
      runtime.getRecovery?.().catch(() => null) ?? null,
    ]);
    publish({ currentVersion: version, componentStatus, recovery });
    await checkIfStale();
  };

  const downloadAndInstall = async () => {
    const update = availableUpdate;
    if (!update || disposed || snapshot.phase !== "available") return false;
    let failureStage: UpdateFailureStage = "download";
    let downloaded = 0;
    let contentLength: number | undefined;
    publish({ phase: "downloading", message: "正在下载更新并验证签名；此时尚未开始安装……", progress: null });
    const onDownload = (event: UpdateDownloadEvent) => {
      if (event.event === "Started") contentLength = event.data.contentLength;
      if (event.event === "Progress") downloaded += event.data.chunkLength;
      if (event.event === "Finished") publish({ progress: 100 });
      else if (contentLength) publish({ progress: Math.min(99, Math.round((downloaded / contentLength) * 100)) });
    };
    try {
      await update.download(onDownload, { timeout: 10 * 60_000 });
      publish({ phase: "downloaded", progress: 100, message: "下载完成且签名有效，正在启动安装程序……" });
      failureStage = "install";
      publish({ phase: "installing", message: "安装程序正在更新主程序与 MCP；只有完整成功后才会重新打开 TodoList。暂时不要再次启动 TodoList。" });
      await update.install({ restartAfterInstall: true });
      await runtime.relaunch?.();
      return true;
    } catch (error) {
      showFailure(error, failureStage);
      return false;
    }
  };

  const retryRetainedUpdate = async () => {
    const recovery = snapshot.recovery;
    if (!recovery || !runtime.retryRecovery || disposed) return;
    publish({ phase: "installing", message: "正在重新启动保留的安装程序；成功前不会宣称更新完成。" });
    try {
      await runtime.retryRecovery(recovery.attemptId);
    } catch (error) {
      showFailure(error, "install");
    }
  };

  const discardRetainedUpdate = async () => {
    const recovery = snapshot.recovery;
    if (!recovery || !runtime.discardRecovery || disposed) return;
    try {
      await runtime.discardRecovery(recovery.attemptId);
      publish({ recovery: null, message: "已清理这次未完成更新的专属缓存。" });
    } catch (error) {
      showFailure(error, "install");
    }
  };

  const openRetainedUpdateLocation = async () => {
    const recovery = snapshot.recovery;
    if (!recovery || !runtime.openRecoveryLocation || disposed) return;
    try {
      await runtime.openRecoveryLocation(recovery.attemptId);
    } catch (error) {
      showFailure(error, "install");
    }
  };

  const dispose = async () => {
    if (disposed) return;
    disposed = true;
    listeners.clear();
    await closeAvailableUpdate();
  };

  return {
    getSnapshot: () => snapshot,
    subscribe(listener: (next: SoftwareUpdateSnapshot) => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    initialize,
    checkForUpdate,
    checkIfStale,
    downloadAndInstall,
    retryRetainedUpdate,
    discardRetainedUpdate,
    openRetainedUpdateLocation,
    dispose,
  };
}

export type SoftwareUpdateManager = ReturnType<typeof createSoftwareUpdateManager>;
