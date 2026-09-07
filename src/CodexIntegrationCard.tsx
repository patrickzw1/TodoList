import { isTauri, invoke } from "@tauri-apps/api/core";
import { CheckCircle, LinkSimple, WarningCircle, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import {
  codexIntegrationActionNotice,
  codexIntegrationPresentation,
  type CodexIntegrationStatus,
} from "./codex-integration-presentation";

export type { CodexIntegrationStatus } from "./codex-integration-presentation";

type PendingAction = "configure" | "remove" | null;

export const browserCodexIntegrationStatus: CodexIntegrationStatus = {
  state: "not_configured",
  configured: false,
  canConfigure: false,
  managedMigration: false,
  reason: "browser_preview",
  pendingUpdates: [],
  actionResult: "",
  updatedItems: [],
  configPath: "~/.codex/config.toml",
  skillPath: "~/.agents/skills/todolist-mcp",
  mcpCommand: "TodoList 安装目录/todolist-mcp",
  message: "网页预览不会修改 Codex 配置",
};

export async function readCodexIntegrationStatus() {
  if (!isTauri()) return browserCodexIntegrationStatus;
  return invoke<CodexIntegrationStatus>("codex_integration_status");
}

function friendlyError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function CodexIntegrationCard({ onStatusChange }: { onStatusChange?: (status: CodexIntegrationStatus) => void }) {
  const desktop = isTauri();
  const [status, setStatus] = useState<CodexIntegrationStatus>(browserCodexIntegrationStatus);
  const [pending, setPending] = useState<PendingAction>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const refresh = async () => {
    try {
      const next = await readCodexIntegrationStatus();
      setStatus(next);
      onStatusChange?.(next);
      setError("");
      setNotice("");
    } catch (reason) {
      setError(friendlyError(reason));
    }
  };

  useEffect(() => { void refresh(); }, [desktop, onStatusChange]);

  const runAction = async () => {
    if (!pending) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const command = pending === "configure"
        ? "configure_codex_integration"
        : "remove_codex_integration";
      const next = await invoke<CodexIntegrationStatus>(command);
      setStatus(next);
      onStatusChange?.(next);
      setPending(null);
      setNotice(codexIntegrationActionNotice(next));
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const presentation = codexIntegrationPresentation(status);

  return (
    <section className={`integration-card state-${status.state}`}>
      <div className="integration-heading">
        <div>
          <strong>Codex 集成</strong>
          <span>{status.message}</span>
        </div>
        <span className={`integration-badge state-${status.state}`}>{presentation.badge}</span>
      </div>

      <div className="integration-paths">
        <div><span>MCP 注册</span><code>{status.configPath}</code></div>
        <div><span>Skill 安装</span><code>{status.skillPath}</code></div>
        <div><span>MCP 程序</span><code>{status.mcpCommand}</code></div>
      </div>

      {status.state === "conflict" && <div className="integration-warning"><WarningCircle />检测到不是本安装管理的同名配置，TodoList 不会覆盖它。</div>}
      {error && <div className="integration-warning"><WarningCircle />{error}</div>}
      {!pending && notice && <div className="integration-success"><CheckCircle weight="fill" />{notice}</div>}

      {pending && (
        <div className="integration-confirm">
          <button className="icon-button" onClick={() => setPending(null)} aria-label="取消"><X /></button>
          <strong>{pending === "configure" ? presentation.confirmTitle : "确认移除 Codex 集成？"}</strong>
          <p>{pending === "configure"
            ? presentation.confirmMessage
            : "只移除由当前 TodoList 安装创建的配置；其他 Codex 配置和未知文件会保留。"}</p>
          <button className="integration-primary" disabled={busy} onClick={() => void runAction()}>{busy ? "处理中……" : "确认"}</button>
        </div>
      )}

      {!pending && (
        <div className="integration-actions">
          {status.configured
            ? <button onClick={() => setPending("remove")}>移除集成</button>
            : <button className="integration-primary" disabled={!desktop || !status.canConfigure} onClick={() => setPending("configure")}><LinkSimple />{presentation.actionLabel}</button>}
          <span>{status.configured ? <><CheckCircle weight="fill" />{presentation.footer}</> : presentation.footer}</span>
        </div>
      )}
    </section>
  );
}
