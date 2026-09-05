import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { ArrowClockwise, DownloadSimple } from "@phosphor-icons/react";
import { useEffect, useState } from "react";

type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "latest" | "error";

function friendlyUpdateError(error: unknown) {
  const detail = error instanceof Error ? error.message : String(error);
  if (/endpoint|url|configuration|config/i.test(detail)) {
    return "当前开发版尚未配置正式更新源。发布到 GitHub 后即可启用。";
  }
  return "检查更新失败，请确认网络连接后重试。";
}

export function SoftwareUpdateCard() {
  const desktop = isTauri();
  const [currentVersion, setCurrentVersion] = useState("0.1.0");
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [message, setMessage] = useState(desktop ? "仅在你点击检查时访问更新源" : "网页预览不执行桌面更新");
  const [progress, setProgress] = useState<number | null>(null);

  useEffect(() => {
    if (desktop) void getVersion().then(setCurrentVersion);
  }, [desktop]);

  useEffect(() => () => { if (availableUpdate) void availableUpdate.close(); }, [availableUpdate]);

  const checkForUpdate = async () => {
    setPhase("checking");
    setMessage("正在安全检查更新……");
    setProgress(null);
    try {
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
      setPhase("error");
      setMessage(friendlyUpdateError(error));
    }
  };

  const installUpdate = async () => {
    if (!availableUpdate) return;
    setPhase("downloading");
    setMessage("正在下载更新，完成后将重启 TodoList……");
    let downloaded = 0;
    let contentLength: number | undefined;
    const onDownload = (event: DownloadEvent) => {
      if (event.event === "Started") contentLength = event.data.contentLength;
      if (event.event === "Progress") downloaded += event.data.chunkLength;
      if (event.event === "Finished") setProgress(100);
      else if (contentLength) setProgress(Math.min(99, Math.round((downloaded / contentLength) * 100)));
    };
    try {
      await availableUpdate.downloadAndInstall(onDownload, { timeout: 10 * 60_000 });
      await relaunch();
    } catch (error) {
      setPhase("error");
      setMessage(friendlyUpdateError(error));
      setProgress(null);
    }
  };

  const busy = phase === "checking" || phase === "downloading";
  const actionLabel = phase === "available"
    ? "下载并重启"
    : phase === "checking"
      ? "检查中……"
      : phase === "downloading"
        ? progress === null ? "下载中……" : `下载 ${progress}%`
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
    </>
  );
}
