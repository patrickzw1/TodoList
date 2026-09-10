export type CodexIntegrationState =
  | "configured"
  | "conflict"
  | "partial"
  | "update_available"
  | "not_configured"
  | "development";

export interface CodexIntegrationStatus {
  state: CodexIntegrationState;
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
}

export function codexIntegrationPresentation(status: CodexIntegrationStatus) {
  if (status.state === "update_available") {
    const items = status.pendingUpdates.length ? status.pendingUpdates.join("、") : "新版集成内容";
    return {
      badge: "可更新",
      actionLabel: "更新集成",
      confirmTitle: "更新 Codex 集成？",
      confirmMessage: `安装路径未变。将同步：${items}；其他 Codex 配置与自定义工具权限保持不变。完成后请重新连接 MCP 或重启 Codex。`,
      footer: "不会自动更新，需要你确认后才会同步",
    };
  }
  if (status.managedMigration) {
    return {
      badge: "可迁移",
      actionLabel: "迁移到当前安装",
      confirmTitle: "迁移到当前安装？",
      confirmMessage: "只把 TodoList 自己登记的 MCP 路径更新为当前安装位置；工具权限与其他 Codex 配置保持不变。完成后请重新连接 MCP 或重启 Codex。",
      footer: "路径迁移只在你确认后执行",
    };
  }
  if (status.state === "configured") {
    return {
      badge: "配置已同步",
      actionLabel: "",
      confirmTitle: "",
      confirmMessage: "",
      footer: "仅表示磁盘配置；当前连接状态请在 Codex 中确认",
    };
  }
  if (status.state === "conflict") {
    return {
      badge: "存在冲突",
      actionLabel: "无法自动配置",
      confirmTitle: "",
      confirmMessage: "",
      footer: "未知配置保持原样",
    };
  }
  if (status.state === "development") {
    return {
      badge: "开发专用",
      actionLabel: "使用项目内 todolist_dev",
      confirmTitle: "",
      confirmMessage: "",
      footer: "开发版不能更改全局 Codex 集成",
    };
  }
  return {
    badge: status.state === "partial" ? "配置不完整" : "未配置",
    actionLabel: "配置 Codex 集成",
    confirmTitle: "确认配置 Codex 集成？",
    confirmMessage: `${status.message} 只写入 TodoList 自己的 MCP 配置和托管 Skill；其他 Codex 配置与自定义工具权限保持不变。完成后请重新连接 MCP 或重启 Codex。`,
    footer: "配置完全由用户主动触发",
  };
}

export function codexIntegrationActionNotice(status: CodexIntegrationStatus) {
  const items = status.updatedItems.join("、");
  if (status.actionResult === "updated") {
    return `集成已更新${items ? `：${items}` : ""}。请重新连接 MCP 或重启 Codex 后加载。`;
  }
  if (status.actionResult === "migrated") {
    return `集成路径已迁移${items ? `：${items}` : ""}。请重新连接 MCP 或重启 Codex 后加载。`;
  }
  if (status.actionResult === "configured") {
    return `集成已配置${items ? `：${items}` : ""}。请重新连接 MCP 或重启 Codex 后加载。`;
  }
  if (status.actionResult === "removed") return "TodoList 管理的 Codex 集成已移除。";
  return "";
}

export function sidebarCodexIntegrationPresentation(
  status: CodexIntegrationStatus | null,
  error: string,
) {
  if (error) return { state: "error", label: "Codex 集成状态读取失败" };
  if (!status) return { state: "checking", label: "正在检查 Codex 集成" };
  if (status.state === "development") return { state: "development", label: "开发 MCP · todolist_dev" };
  if (status.state === "configured") return { state: "configured", label: "Codex 集成配置已同步" };
  if (status.state === "update_available") return { state: "update-available", label: "Codex 集成有新版可同步" };
  if (status.state === "partial") {
    return status.managedMigration
      ? { state: "partial", label: "Codex 集成路径待迁移" }
      : { state: "partial", label: "Codex 集成配置不完整" };
  }
  if (status.state === "conflict") return { state: "conflict", label: "Codex 集成存在冲突" };
  return {
    state: "not-configured",
    label: status.canConfigure ? "Codex 集成未配置" : "网页预览 · 集成不可用",
  };
}

export function hasCodexIntegrationUpdate(status: CodexIntegrationStatus | null) {
  return status?.state === "update_available";
}
