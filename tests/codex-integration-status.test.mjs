import assert from "node:assert/strict";
import test from "node:test";
import {
  codexIntegrationActionNotice,
  codexIntegrationPresentation,
  hasCodexIntegrationUpdate,
  sidebarCodexIntegrationPresentation,
} from "../src/codex-integration-presentation.ts";

function status(overrides = {}) {
  return {
    state: "configured",
    configured: true,
    canConfigure: true,
    managedMigration: false,
    reason: "up_to_date",
    pendingUpdates: [],
    actionResult: "",
    updatedItems: [],
    configPath: "C:/Users/test/.codex/config.toml",
    skillPath: "C:/Users/test/.agents/skills/todolist-mcp",
    mcpCommand: "C:/Program Files/TodoList/todolist-mcp.exe",
    message: "TodoList 的磁盘配置已同步；这不代表 Codex 当前连接状态。",
    ...overrides,
  };
}

test("same-path managed updates use neutral update copy and an explicit update action", () => {
  const update = status({
    state: "update_available",
    configured: false,
    reason: "managed_update",
    pendingUpdates: ["新版使用说明", "工具列表（新增 get_task_activity）"],
    message: "安装路径未变，需要同步新版使用说明和工具列表。",
  });
  const presentation = codexIntegrationPresentation(update);

  assert.equal(presentation.badge, "可更新");
  assert.equal(presentation.actionLabel, "更新集成");
  assert.equal(presentation.confirmTitle, "更新 Codex 集成？");
  assert.match(presentation.confirmMessage, /安装路径未变/);
  assert.match(presentation.confirmMessage, /get_task_activity/);
  assert.deepEqual(sidebarCodexIntegrationPresentation(update, ""), {
    state: "update-available",
    label: "Codex 集成有新版可同步",
  });
  assert.equal(hasCodexIntegrationUpdate(update), true);
});

test("sidebar update badge appears only for a real managed integration update", () => {
  assert.equal(hasCodexIntegrationUpdate(null), false);
  for (const state of ["configured", "not_configured", "partial", "conflict", "development"]) {
    assert.equal(hasCodexIntegrationUpdate(status({ state })), false, state);
  }
  assert.equal(hasCodexIntegrationUpdate(status({ state: "update_available" })), true);
  assert.equal(hasCodexIntegrationUpdate(status({ state: "configured", actionResult: "updated" })), false);
});

test("path migration never claims that the path is unchanged", () => {
  const migration = status({
    state: "partial",
    configured: false,
    managedMigration: true,
    reason: "path_migration",
  });
  const presentation = codexIntegrationPresentation(migration);

  assert.equal(presentation.actionLabel, "迁移到当前安装");
  assert.doesNotMatch(presentation.confirmMessage, /路径未变/);
  assert.match(presentation.confirmMessage, /工具权限.*保持不变/);
});

test("update success lists the actual changed items and reconnect guidance", () => {
  const updated = status({
    actionResult: "updated",
    updatedItems: ["新版使用说明", "工具列表（新增 get_task_activity）"],
  });
  const notice = codexIntegrationActionNotice(updated);

  assert.match(notice, /^集成已更新/);
  assert.match(notice, /新版使用说明/);
  assert.match(notice, /get_task_activity/);
  assert.match(notice, /重新连接 MCP 或重启 Codex/);
});

test("configured copy describes disk state instead of runtime connection health", () => {
  const configured = status();
  const presentation = codexIntegrationPresentation(configured);

  assert.equal(presentation.badge, "配置已同步");
  assert.match(presentation.footer, /仅表示磁盘配置/);
  assert.deepEqual(sidebarCodexIntegrationPresentation(configured, ""), {
    state: "configured",
    label: "Codex 集成配置已同步",
  });
});

test("missing configuration, conflict, and read errors remain distinct", () => {
  const missing = status({ state: "partial", configured: false, reason: "missing_skill" });
  const conflict = status({ state: "conflict", configured: false, canConfigure: false, reason: "unknown_config" });

  assert.equal(codexIntegrationPresentation(missing).badge, "配置不完整");
  assert.equal(codexIntegrationPresentation(conflict).badge, "存在冲突");
  assert.deepEqual(sidebarCodexIntegrationPresentation(null, "permission denied"), {
    state: "error",
    label: "Codex 集成状态读取失败",
  });
});
