import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { ArrowClockwise, DownloadSimple, FolderOpen, Trash } from "@phosphor-icons/react";
import { createContext, type ReactNode, useContext, useEffect, useMemo, useRef, useState } from "react";
import {
  createSoftwareUpdateManager, UPDATE_CHECK_INTERVAL_MS, type ManagedSoftwareUpdate,
  type SoftwareUpdateManager, type SoftwareUpdateRuntime, type SoftwareUpdateSnapshot,
  type UpdateDownloadEvent,
} from "./software-update-manager";

function managedUpdate(update: Update): ManagedSoftwareUpdate {
  return {
    version: update.version,
    download: (onEvent, options) => update.download((event) => onEvent(event as UpdateDownloadEvent), options),
    install: (options) => update.install(options),
    close: () => update.close(),
  };
}

function createRuntime(): SoftwareUpdateRuntime {
  const desktop = isTauri();
  return {
    desktop,
    buildVersion: __APP_VERSION__,
    ...(desktop ? {
      getCurrentVersion: getVersion,
      check: async () => {
        const update = await check({ timeout: 30_000, allowDowngrades: false });
        return update ? managedUpdate(update) : null;
      },
      getComponentStatus: () => invoke("component_build_status"),
      getRecovery: () => invoke("update_recovery_status"),
      retryRecovery: (attemptId) => invoke("retry_update_installer", { attemptId }),
      discardRecovery: (attemptId) => invoke("discard_update_installer", { attemptId }),
      openRecoveryLocation: (attemptId) => invoke("open_update_installer_location", { attemptId }),
      relaunch,
    } : {}),
  };
}

type SoftwareUpdateContextValue = SoftwareUpdateSnapshot & {
  checkForUpdate: SoftwareUpdateManager["checkForUpdate"];
  checkIfStale: SoftwareUpdateManager["checkIfStale"];
  downloadAndInstall: SoftwareUpdateManager["downloadAndInstall"];
  retryRetainedUpdate: SoftwareUpdateManager["retryRetainedUpdate"];
  discardRetainedUpdate: SoftwareUpdateManager["discardRetainedUpdate"];
  openRetainedUpdateLocation: SoftwareUpdateManager["openRetainedUpdateLocation"];
};

const SoftwareUpdateContext = createContext<SoftwareUpdateContextValue | null>(null);

export function SoftwareUpdateProvider({ children }: { children: ReactNode }) {
  const manager = useMemo(() => createSoftwareUpdateManager(createRuntime()), []);
  const [snapshot, setSnapshot] = useState(manager.getSnapshot);
  const disposeTimer = useRef<number | null>(null);

  useEffect(() => {
    if (disposeTimer.current !== null) window.clearTimeout(disposeTimer.current);
    disposeTimer.current = null;
    const unsubscribe = manager.subscribe(setSnapshot);
    void manager.initialize();
    if (!manager.getSnapshot().desktop) return unsubscribe;

    const checkWhenVisible = () => {
      if (document.visibilityState === "visible") void manager.checkIfStale();
    };
    const timer = window.setInterval(() => void manager.checkIfStale(), UPDATE_CHECK_INTERVAL_MS);
    window.addEventListener("focus", checkWhenVisible);
    document.addEventListener("visibilitychange", checkWhenVisible);
    return () => {
      unsubscribe();
      window.clearInterval(timer);
      window.removeEventListener("focus", checkWhenVisible);
      document.removeEventListener("visibilitychange", checkWhenVisible);
      // React StrictMode immediately mounts the same provider again in development.
      // Delay disposal by one task so that cycle can retain the single manager.
      disposeTimer.current = window.setTimeout(() => void manager.dispose(), 0);
    };
  }, [manager]);

  const value = useMemo<SoftwareUpdateContextValue>(() => ({
    ...snapshot,
    checkForUpdate: manager.checkForUpdate,
    checkIfStale: manager.checkIfStale,
    downloadAndInstall: manager.downloadAndInstall,
    retryRetainedUpdate: manager.retryRetainedUpdate,
    discardRetainedUpdate: manager.discardRetainedUpdate,
    openRetainedUpdateLocation: manager.openRetainedUpdateLocation,
  }), [manager, snapshot]);

  return <SoftwareUpdateContext.Provider value={value}>{children}</SoftwareUpdateContext.Provider>;
}

export function useSoftwareUpdate() {
  const context = useContext(SoftwareUpdateContext);
  if (!context) throw new Error("useSoftwareUpdate must be used inside SoftwareUpdateProvider");
  return context;
}

export function SoftwareUpdateCard() {
  const update = useSoftwareUpdate();
  const busy = ["checking", "downloading", "downloaded", "installing"].includes(update.phase);
  const actionLabel = update.phase === "available"
    ? "下载并重启"
    : update.phase === "checking"
      ? "检查中……"
      : update.phase === "downloading"
        ? update.progress === null ? "下载中……" : `下载 ${update.progress}%`
        : update.phase === "downloaded" ? "准备安装……"
        : update.phase === "installing" ? "安装中……"
        : update.phase === "latest" ? "再次检查" : "检查更新";

  return (
    <>
      <div className="settings-card update-card">
        <div>
          <strong>软件更新</strong>
          <span>当前版本 {update.currentVersion}{update.availableVersion ? ` · 可用版本 ${update.availableVersion}` : ""} · 不允许降级安装</span>
        </div>
        <button
          disabled={!update.desktop || busy}
          onClick={() => void (update.phase === "available" ? update.downloadAndInstall() : update.checkForUpdate())}
        >
          {update.phase === "available" ? <DownloadSimple /> : <ArrowClockwise />}
          {actionLabel}
        </button>
      </div>
      <div className={`update-message ${update.phase === "error" ? "is-error" : ""}`}>
        {update.message}
        {update.progress !== null && update.phase === "downloading" && <progress max="100" value={update.progress} />}
      </div>
      {update.componentStatus && <div className={`update-component-state ${update.componentStatus.matches ? "is-matched" : "is-error"}`}>
        {update.componentStatus.message}
      </div>}
      {update.recovery && <div className={`update-recovery ${update.recovery.state === "failed" ? "is-error" : ""}`}>
        <div>
          <strong>{update.recovery.state === "failed" ? `版本 ${update.recovery.version} 安装失败` : `版本 ${update.recovery.version} 安装已取消`}</strong>
          <span>{update.recovery.reason}</span>
          <small title={update.recovery.installerDirectory}>安装包保留在 TodoList 专属临时目录，可重试或手动清理。</small>
        </div>
        <div className="update-recovery-actions">
          <button onClick={() => void update.retryRetainedUpdate()} disabled={busy}><ArrowClockwise />重试</button>
          <button onClick={() => void update.openRetainedUpdateLocation()}><FolderOpen />打开位置</button>
          <button className="danger-text" onClick={() => void update.discardRetainedUpdate()} disabled={busy}><Trash />清理</button>
        </div>
      </div>}
    </>
  );
}
