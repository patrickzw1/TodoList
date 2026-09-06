import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { ArrowClockwise, DownloadSimple, FolderOpen, Trash } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { friendlyUpdateError, type UpdateFailureStage } from "./update-status";

type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "downloaded" | "installing" | "latest" | "error";
type ComponentBuildStatus = { desktopBuild: string; mcpBuild?: string; matches: boolean; message: string };
type UpdateRecovery = { attemptId: string; version: string; state: "failed" | "cancelled"; reason: string; installerDirectory: string; explorerOpened: boolean };

export function SoftwareUpdateCard() {
  const desktop = isTauri();
  const [currentVersion, setCurrentVersion] = useState("0.1.0");
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [message, setMessage] = useState(desktop ? "仅在你点击检查时访问更新源" : "网页预览不执行桌面更新");
  const [progress, setProgress] = useState<number | null>(null);
  const [componentStatus, setComponentStatus] = useState<ComponentBuildStatus | null>(null);
  const [recovery, setRecovery] = useState<UpdateRecovery | null>(null);

  useEffect(() => {
    if (!desktop) return;
    void getVersion().then(setCurrentVersion);
    void invoke<ComponentBuildStatus>("component_build_status").then(setComponentStatus).catch(() => setComponentStatus(null));
    void invoke<UpdateRecovery | null>("update_recovery_status").then(setRecovery).catch(() => setRecovery(null));
  }, [desktop]);

  useEffect(() => () => { if (availableUpdate) void availableUpdate.close(); }, [availableUpdate]);

  const showFailure = (error: unknown, stage: UpdateFailureStage) => {
    setPhase("error");
    setMessage(friendlyUpdateError(error, stage));
    setProgress(null);
  };

  const checkForUpdate = async () => {
    setPhase("checking");
    setMessage("正在安全检查更新……");
    setProgress(null);
    try {
      if (availableUpdate) await availableUpdate.close();
      const update = await check({ timeout: 30_000, allowDowngrades: false });
      setAvailableUpdate(update);
      if (update) {
        setPhase("available");
        setMessage(`发现新版本 ${update.version}，安装前会验证更新签名。`);
      } else {
        setPhase("latest");
        setMessage("当前已经是最新版本。");
      }
    } catch (error) {
      showFailure(error, "check");
    }
  };

  const installUpdate = async () => {
    if (!availableUpdate) return;
    let failureStage: UpdateFailureStage = "download";
    setPhase("downloading");
    setMessage("正在下载更新并验证签名；此时尚未开始安装……");
    let downloaded = 0;
    let contentLength: number | undefined;
    const onDownload = (event: DownloadEvent) => {
      if (event.event === "Started") contentLength = event.data.contentLength;
      if (event.event === "Progress") downloaded += event.data.chunkLength;
      if (event.event === "Finished") setProgress(100);
      else if (contentLength) setProgress(Math.min(99, Math.round((downloaded / contentLength) * 100)));
    };
    try {
      await availableUpdate.download(onDownload, { timeout: 10 * 60_000 });
      setPhase("downloaded");
      setProgress(100);
      setMessage("下载完成且签名有效，正在启动安装程序……");
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      failureStage = "install";
      setPhase("installing");
      setMessage("安装程序正在更新主程序与 MCP；只有完整成功后才会重新打开 TodoList。暂时不要再次启动 TodoList。" );
      await availableUpdate.install({ restartAfterInstall: true });
      await relaunch();
    } catch (error) {
      showFailure(error, failureStage);
    }
  };

  const retryRetainedUpdate = async () => {
    if (!recovery) return;
    setPhase("installing");
    setMessage("正在重新启动保留的安装程序；成功前不会宣称更新完成。" );
    try {
      await invoke("retry_update_installer", { attemptId: recovery.attemptId });
    } catch (error) {
      showFailure(error, "install");
    }
  };

  const discardRetainedUpdate = async () => {
    if (!recovery) return;
    try {
      await invoke("discard_update_installer", { attemptId: recovery.attemptId });
      setRecovery(null);
      setMessage("已清理这次未完成更新的专属缓存。" );
    } catch (error) {
      showFailure(error, "install");
    }
  };

  const busy = phase === "checking" || phase === "downloading" || phase === "downloaded" || phase === "installing";
  const actionLabel = phase === "available"
    ? "下载并重启"
    : phase === "checking"
      ? "检查中……"
      : phase === "downloading"
        ? progress === null ? "下载中……" : `下载 ${progress}%`
        : phase === "downloaded" ? "准备安装……"
        : phase === "installing" ? "安装中……"
        : phase === "latest" ? "再次检查" : "检查更新";

  return (
    <>
      <div className="settings-card update-card">
        <div>
          <strong>软件更新</strong>
          <span>当前版本 {currentVersion} · 不允许降级安装</span>
        </div>
        <button
          disabled={!desktop || busy}
          onClick={() => void (phase === "available" ? installUpdate() : checkForUpdate())}
        >
          {phase === "available" ? <DownloadSimple /> : <ArrowClockwise />}
          {actionLabel}
        </button>
      </div>
      <div className={`update-message ${phase === "error" ? "is-error" : ""}`}>
        {message}
        {progress !== null && phase === "downloading" && <progress max="100" value={progress} />}
      </div>
      {componentStatus && <div className={`update-component-state ${componentStatus.matches ? "is-matched" : "is-error"}`}>
        {componentStatus.message}
      </div>}
      {recovery && <div className={`update-recovery ${recovery.state === "failed" ? "is-error" : ""}`}>
        <div>
          <strong>{recovery.state === "failed" ? `版本 ${recovery.version} 安装失败` : `版本 ${recovery.version} 安装已取消`}</strong>
          <span>{recovery.reason}</span>
          <small title={recovery.installerDirectory}>安装包保留在 TodoList 专属临时目录，可重试或手动清理。</small>
        </div>
        <div className="update-recovery-actions">
          <button onClick={() => void retryRetainedUpdate()} disabled={busy}><ArrowClockwise />重试</button>
          <button onClick={() => void invoke("open_update_installer_location", { attemptId: recovery.attemptId })}><FolderOpen />打开位置</button>
          <button className="danger-text" onClick={() => void discardRetainedUpdate()} disabled={busy}><Trash />清理</button>
        </div>
      </div>}
    </>
  );
}
