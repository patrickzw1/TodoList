import { invoke, isTauri } from "@tauri-apps/api/core";
import { ArrowClockwise, CheckCircle, LinkSimple, WarningCircle, X } from "@phosphor-icons/react";
import { type KeyboardEvent, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { aiClients, clientLabels, componentStateLabel, integrationActionLabel, integrationBadge, type AIClient, type AIIntegrationStatus, type IntegrationLocations } from "./ai-integration-presentation";
import { createAIIntegrationSession } from "./ai-integration-session";

const defaultPaths: Record<AIClient, [string, string]> = {
  codex: ["~/.codex/config.toml", "~/.agents/skills/todolist-mcp"],
  claude_code: ["~/.claude.json", "~/.claude/skills/todolist-mcp"],
  deepseek_harness: ["$DSH_HOME/cordis.patch.yml（默认 ~/.dsh）", "$DSH_HOME/skills/todolist-mcp"],
  antigravity: ["~/.gemini/config/mcp_config.json", "~/.gemini/config/skills/todolist-mcp"],
};
const tabLabels: Record<AIClient, string> = { codex: "Codex", claude_code: "Claude Code", deepseek_harness: "DeepSeek Harness", antigravity: "Antigravity" };
export function browserIntegrationStatus(client: AIClient): AIIntegrationStatus {
  return { client, label: clientLabels[client], state: "not_configured", configured: false, canConfigure: false, canRemove: false, managedMigration: false, reason: "browser_preview", pendingUpdates: [], actionResult: "", updatedItems: [], configPath: defaultPaths[client][0], skillPath: defaultPaths[client][1], mcpCommand: "TodoList 安装目录/todolist-mcp", detected: false, detection: "网页预览未检测本机客户端。", mcpState: "missing", skillState: "missing", runtimeHealth: "unverified", message: "网页预览不会读取或修改客户端配置。", reconnect: "配置完成后，请重新连接 MCP 或重启所选客户端。" };
}
async function readStatus(client: AIClient, locations?: IntegrationLocations) {
  return isTauri() ? invoke<AIIntegrationStatus>("ai_integration_status", { client, locations }) : browserIntegrationStatus(client);
}
export function readAIIntegrationStatuses() { return Promise.all(aiClients.map((client) => readStatus(client))); }

export function AIIntegrationCard({ onStatusChange }: { onStatusChange: (status: AIIntegrationStatus) => void }) {
  const desktop = isTauri();
  const statusChange = useRef(onStatusChange);
  statusChange.current = onStatusChange;
  const [session] = useState(() => createAIIntegrationSession(aiClients.map(browserIntegrationStatus), {
    desktop, readStatus,
    mutate: (action, client, locations) => invoke<AIIntegrationStatus>(action === "configure" ? "configure_ai_integration" : "remove_ai_integration", { client, locations }),
    onStatusChange: (next) => statusChange.current(next),
  }));
  const { client, contexts, pending, busy } = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const { status, manual, draft, error, notice } = contexts[client];
  useEffect(() => { void session.initialize(); }, [session]);
  const moveTab = (event: KeyboardEvent<HTMLDivElement>) => {
    if (busy) return;
    const index = aiClients.indexOf(client);
    const last = aiClients.length - 1;
    const next = event.key === "Home" ? 0 : event.key === "End" ? last
      : event.key === "ArrowRight" ? (index + 1) % aiClients.length
      : event.key === "ArrowLeft" ? (index + last) % aiClients.length : null;
    if (next === null) return;
    event.preventDefault();
    session.select(aiClients[next]);
    event.currentTarget.querySelectorAll("button")[next]?.focus();
  };
  return <div className="ai-integration">
    <div className="integration-client-tabs" role="tablist" aria-label="选择 AI 客户端" onKeyDown={moveTab}>
      {aiClients.map((id) => <button key={id} id={`client-tab-${id}`} type="button" role="tab" tabIndex={client === id ? 0 : -1} aria-selected={client === id} aria-controls="client-integration-panel" disabled={busy} onClick={() => session.select(id)}>{tabLabels[id]}<small>{integrationBadge(contexts[id].status)}</small></button>)}
    </div>
    <section id="client-integration-panel" role="tabpanel" aria-labelledby={`client-tab-${client}`} className={`integration-card state-${status.state}`}>
      <div className="integration-heading"><div><strong>{status.label}</strong><span>{status.message}</span></div><span className={`integration-badge state-${status.state}`}>{integrationBadge(status)}</span></div>
      <p className="integration-detection">{status.detection}</p>
      <div className="integration-components"><span>MCP 注册 <b>{status.mcpState === "missing" ? "未注册" : componentStateLabel(status.mcpState)}</b></span><span>Skill 文件 <b>{componentStateLabel(status.skillState)}</b></span><span>运行连接 <b>未验证</b></span></div>
      <div className="integration-paths">
        <div><span>配置文件</span><code>{status.configPath}</code></div>
        <div><span>Skill 目录</span><code>{status.skillPath}<br />SKILL.md</code></div>
        <div><span>MCP 程序</span><code>{status.mcpCommand}</code></div>
      </div>
      <div className="integration-path-actions"><button disabled={busy} onClick={() => void session.refresh()}><ArrowClockwise />刷新状态</button><button disabled={busy} onClick={() => void session.toggleManual()}>{manual ? "使用自动路径" : "手动指定路径"}</button></div>
      {manual && <div className="integration-manual">
        <label>所选客户端的 MCP 配置文件<input disabled={busy} value={draft.configPath} placeholder="绝对路径" onChange={(e) => session.editDraft("configPath", e.target.value)} /></label>
        <label>所选客户端的 Skill 目录<input disabled={busy} value={draft.skillPath} placeholder="绝对路径，以 todolist-mcp 目录结尾" onChange={(e) => session.editDraft("skillPath", e.target.value)} /></label>
        <p>{client === "antigravity" ? "可从 Antigravity 的 Copy Config File Path 获取实际配置路径。App 与 CLI 共用全局 Skill。" : "请选实际由客户端加载的位置。检查路径只读取文件。"}</p>
        <button disabled={!desktop || busy || !draft.configPath.trim() || !draft.skillPath.trim()} onClick={() => void session.refresh()}>检查这些路径</button>
      </div>}
      {status.state === "conflict" && <div className="integration-warning"><WarningCircle />同名配置或 Skill 未被可靠识别为托管内容。</div>}
      {error && <div className="integration-warning" role="alert"><WarningCircle />{error}</div>}
      {notice && <div className="integration-success" role="status"><CheckCircle weight="fill" />{notice}</div>}
      {pending && <div className="integration-confirm">
        <button className="icon-button" disabled={busy} onClick={session.cancel} aria-label="取消确认"><X /></button>
        <strong>{pending.action === "remove" ? "移除" : integrationActionLabel(status)} {status.label} 的 TodoList 集成？</strong>
        <p>{pending.action === "remove" ? "将移除以上路径中 TodoList 自己管理的 MCP 条目和 Skill 文件，并保存备份。" : `将写入以上配置与 Skill 路径，并保存已有文件备份。${status.pendingUpdates.length ? `同步项目：${status.pendingUpdates.join("、")}。` : ""}`}{status.reconnect}</p>
        <button className="integration-primary" disabled={!session.canAct(pending.action)} onClick={() => void session.run()}>{busy ? "处理中……" : `确认${pending.action === "remove" ? "移除" : integrationActionLabel(status)}`}</button>
      </div>}
      {!pending && <div className="integration-actions">
        <div className="integration-action-buttons">{!status.configured && <button className="integration-primary" disabled={!session.canAct("configure")} onClick={() => session.confirm("configure")}><LinkSimple />{integrationActionLabel(status)}</button>}{status.canRemove && <button disabled={!session.canAct("remove")} onClick={() => session.confirm("remove")}>移除集成</button>}</div>
      </div>}
      <p className="integration-reconnect">{status.reconnect}</p>
    </section>
  </div>;
}
