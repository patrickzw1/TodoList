export const aiClients = ["codex", "claude_code", "deepseek_harness"] as const;
export type AIClient = typeof aiClients[number];
export type AIIntegrationState = "configured" | "conflict" | "partial" | "update_available" | "not_configured" | "development" | "client_missing" | "error";
export interface AIIntegrationStatus {
  client: AIClient;
  label: string;
  state: AIIntegrationState;
  configured: boolean;
  canConfigure: boolean;
  managedMigration: boolean;
  reason: string;
  pendingUpdates: string[];
  actionResult: "" | "configured" | "updated" | "migrated" | "removed";
  updatedItems: string[];
  configPath: string;
  skillPath: string;
  mcpCommand: string;
  message: string;
  detected: boolean;
  detection: string;
  canRemove: boolean;
  mcpState: string;
  skillState: string;
  runtimeHealth: "unverified";
  reconnect: string;
}
export type IntegrationLocations = { configPath?: string; skillPath?: string };
export const clientLabels: Record<AIClient, string> = { codex: "Codex", claude_code: "Claude Code (CC)", deepseek_harness: "DeepSeek Harness (DSH)" };
export function integrationBadge(status: AIIntegrationStatus) {
  const badges = { configured: "配置已同步", conflict: "存在冲突", partial: "配置不完整", update_available: "可更新", not_configured: "未配置", development: "开发专用", client_missing: "未发现客户端", error: "读取失败" };
  return status.managedMigration ? "可迁移" : badges[status.state];
}
export function componentStateLabel(state: string) {
  const labels: Record<string, string> = { registered: "已注册", current: "已同步", missing: "未安装", update_available: "可更新", path_migration: "路径待迁移", conflict: "冲突", error: "读取失败", development: "开发专用" };
  return labels[state] ?? "未确认";
}
export function integrationActionLabel(status: AIIntegrationStatus) {
  if (status.managedMigration) return "迁移到当前安装";
  if (status.state === "update_available") return "更新集成";
  if (status.state === "partial") return "补齐集成";
  return "配置集成";
}
export function integrationActionNotice(status: AIIntegrationStatus) {
  if (!status.actionResult) return "";
  const items = status.updatedItems.length ? `：${status.updatedItems.join("、")}` : "";
  if (status.actionResult === "removed") return `${status.label} 的 TodoList 托管集成已移除${items}。`;
  return `${status.label} 集成已${status.actionResult === "migrated" ? "迁移" : status.actionResult === "updated" ? "更新" : "配置"}${items}。${status.reconnect}`;
}
export function hasAIIntegrationUpdate(statuses: AIIntegrationStatus[] | null) {
  return statuses?.some((status) => status.state === "update_available") ?? false;
}
export function sidebarAIIntegrationPresentation(statuses: AIIntegrationStatus[] | null, error: string) {
  if (error) return { state: "error", label: "AI 集成状态读取失败" };
  if (!statuses) return { state: "checking", label: "正在检查 AI 集成" };
  if (statuses.every((s) => s.state === "development")) return { state: "development", label: "开发 MCP · todolist_dev" };
  if (hasAIIntegrationUpdate(statuses)) return { state: "update-available", label: "AI 集成有新版可同步" };
  if (statuses.some((s) => s.state === "error" || s.state === "conflict")) return { state: "conflict", label: "AI 集成有待处理项" };
  if (statuses.some((s) => s.state === "partial")) return { state: "partial", label: "AI 集成配置不完整" };
  const count = statuses.filter((s) => s.configured).length;
  return count ? { state: "configured", label: `${count} 个客户端配置已同步` } : { state: "not-configured", label: "AI 集成未配置" };
}
