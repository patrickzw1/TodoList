import { isTauri, invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { CheckCircle, DownloadSimple, UploadSimple, WarningCircle, X } from "@phosphor-icons/react";
import { ChangeEvent, useRef, useState } from "react";
import { localIsoDate } from "./date-utils";
import type { Workspace } from "./types";
import { backupFileName, backupJson, createWorkspaceBackup, MAX_BACKUP_BYTES, parseWorkspaceBackup, type WorkspaceBackup } from "./workspace-backup";

const filters = [{ name: "TodoList JSON 备份", extensions: ["json"] }];

function backupDate(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp * 1000));
}

function downloadBrowserBackup(workspace: Workspace) {
  const name = `TodoList-backup-${localIsoDate()}.json`;
  const url = URL.createObjectURL(new Blob([backupJson(createWorkspaceBackup(workspace))], { type: "application/json" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
  return name;
}

export function DataBackupCard({ workspace, onImport }: { workspace: Workspace; onImport: (workspace: Workspace) => void }) {
  const fileInput = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState<"export" | "import" | null>(null);
  const [pending, setPending] = useState<{ backup: WorkspaceBackup; source: string } | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const desktop = isTauri();

  const exportBackup = async () => {
    setBusy("export");
    setError("");
    try {
      let name: string;
      if (desktop) {
        const path = await save({ defaultPath: `TodoList-backup-${localIsoDate()}.json`, filters });
        if (!path) return;
        await invoke("export_workspace_backup", { path, workspace });
        name = backupFileName(path);
      } else {
        name = downloadBrowserBackup(workspace);
      }
      setMessage(`已导出 ${name}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const prepareImport = async () => {
    setError("");
    setMessage("");
    if (!desktop) {
      fileInput.current?.click();
      return;
    }
    setBusy("import");
    try {
      const path = await open({ multiple: false, directory: false, filters });
      if (!path) return;
      const backup = await invoke<WorkspaceBackup>("read_workspace_backup", { path });
      setPending({ backup, source: backupFileName(path) });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const selectBrowserFile = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setBusy("import");
    setError("");
    setMessage("");
    try {
      if (file.size > MAX_BACKUP_BYTES) throw new Error("备份超过 25 MB 安全上限");
      setPending({ backup: parseWorkspaceBackup(await file.text()), source: file.name });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const confirmImport = () => {
    if (!pending) return;
    onImport(pending.backup.workspace);
    setMessage(`已载入 ${pending.source}，请确认左侧显示“本地已保存”`);
    setPending(null);
  };

  const archived = pending?.backup.workspace.tasks.filter((task) => task.archived).length ?? 0;
  return (
    <section className="backup-card">
      <div className="backup-heading">
        <div><strong>数据备份与恢复</strong><span>JSON 备份包含项目、任务、子任务、验收状态和活动记录</span></div>
        <div className="backup-actions">
          <button disabled={busy !== null} onClick={() => void exportBackup()}><DownloadSimple />{busy === "export" ? "正在导出……" : "导出备份"}</button>
          <button disabled={busy !== null} onClick={() => void prepareImport()}><UploadSimple />{busy === "import" ? "正在读取……" : "导入备份"}</button>
        </div>
      </div>
      <input ref={fileInput} className="backup-file-input" type="file" accept="application/json,.json" onChange={(event) => void selectBrowserFile(event)} />
      {pending && <div className="backup-preview">
        <button className="icon-button" onClick={() => setPending(null)} aria-label="取消导入"><X /></button>
        <strong>确认替换当前任务库？</strong>
        <span>{pending.source} · {backupDate(pending.backup.exportedAt)}</span>
        <dl><div><dt>项目</dt><dd>{pending.backup.workspace.projects.length}</dd></div><div><dt>任务</dt><dd>{pending.backup.workspace.tasks.length}</dd></div><div><dt>已归档</dt><dd>{archived}</dd></div></dl>
        <p><WarningCircle />确认后会用这份备份替换当前任务库。建议先导出当前数据；原备份文件不会被修改。</p>
        <div><button disabled={busy !== null} onClick={() => void exportBackup()}><DownloadSimple />先备份当前数据</button><button className="backup-confirm" onClick={confirmImport}>确认替换任务库</button></div>
      </div>}
      {message && <div className="backup-message"><CheckCircle weight="fill" />{message}</div>}
      {error && <div className="backup-message is-error"><WarningCircle />{error}</div>}
    </section>
  );
}
