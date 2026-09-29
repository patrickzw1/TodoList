import assert from "node:assert/strict";
import { test } from "node:test";
import { aiClients, hasAIIntegrationUpdate, integrationActionNotice, integrationBadge, sidebarAIIntegrationPresentation } from "../src/ai-integration-presentation.ts";
import { createAIIntegrationSession } from "../src/ai-integration-session.ts";

const status = (client, state = "not_configured") => ({ client, label: client, state, configured: state === "configured", canConfigure: true, canRemove: false, managedMigration: false, pendingUpdates: [], updatedItems: [], actionResult: "", reconnect: "Reconnect in the selected client." });
test("sidebar uses all client disk states without implying runtime connection", () => {
  const statuses = [status("codex", "configured"), status("claude_code", "configured"), status("deepseek_harness", "client_missing")];
  assert.equal(sidebarAIIntegrationPresentation(statuses, "").label, "2 个客户端配置已同步");
  assert.equal(hasAIIntegrationUpdate(statuses), false);
  for (const state of ["partial", "error", "conflict", "client_missing", "development"]) assert.equal(hasAIIntegrationUpdate([status("claude_code", state)]), false);
  statuses[2] = status("deepseek_harness", "update_available");
  assert.equal(hasAIIntegrationUpdate(statuses), true);
  assert.equal(sidebarAIIntegrationPresentation(statuses, "").state, "update-available");
  statuses[2] = status("deepseek_harness", "configured"); assert.equal(hasAIIntegrationUpdate(statuses), false);
});
test("updates and migration remain distinct and report the selected client's actual changes", () => {
  const updated = { ...status("claude_code", "configured"), actionResult: "updated", updatedItems: ["TodoList Skill"] };
  assert.match(integrationActionNotice(updated), /claude_code.*已更新：TodoList Skill/);
  assert.match(integrationActionNotice(updated), /Reconnect in the selected client/);
  assert.equal(integrationBadge({ ...updated, state: "update_available" }), "可更新");
  assert.equal(integrationBadge({ ...updated, state: "partial", managedMigration: true }), "可迁移");
  assert.equal(integrationBadge(status("codex", "client_missing")), "未发现客户端");
});

function integrationFixture() {
  const savedPaths = Object.fromEntries(aiClients.map((client) => [client, {
    configPath: `C:/saved/${client}/config`, skillPath: `C:/saved/${client}/skills/todolist-mcp`,
  }]));
  const inspected = (client, paths = savedPaths[client]) => ({
    ...status(client, "partial"), ...paths, canRemove: true, detected: true, detection: "fixture",
    reason: "", configPath: paths.configPath, skillPath: paths.skillPath, mcpCommand: "C:/fixture/todolist-mcp.exe",
    mcpState: "registered", skillState: "missing", runtimeHealth: "unverified", message: "fixture",
  });
  const reads = [], writes = [];
  let readOverride;
  const session = createAIIntegrationSession(aiClients.map((client) => inspected(client)), {
    desktop: true,
    async readStatus(client, locations) {
      reads.push({ client, locations });
      if (readOverride) return readOverride(client, locations);
      return inspected(client, locations);
    },
    async mutate(action, client, locations) {
      writes.push({ action, client, locations });
      return { ...inspected(client, locations), actionResult: action === "remove" ? "removed" : "configured" };
    },
    onStatusChange() {},
  });
  return { session, savedPaths, reads, writes, inspected, overrideRead: (read) => { readOverride = read; } };
}
const selectedContext = (session) => session.getSnapshot().contexts[session.getSnapshot().client];
const customPaths = (client) => ({ configPath: `C:/unsaved/${client}/config`, skillPath: `C:/unsaved/${client}/skills/todolist-mcp` });
async function checkManual(session, paths) {
  await session.toggleManual();
  session.editDraft("configPath", paths.configPath);
  session.editDraft("skillPath", paths.skillPath);
  await session.refresh();
}

for (const action of ["configure", "remove"]) {
  test(`${action} sends the displayed unsaved custom paths after A → B → A for every client`, async () => {
    for (const client of aiClients) {
      const { session, savedPaths, reads, writes } = integrationFixture();
      await session.initialize();
      session.select(client);
      await checkManual(session, customPaths(client));
      const other = aiClients.find((id) => id !== client);
      session.select(other);
      await checkManual(session, customPaths(other));
      session.select(client);
      const context = selectedContext(session);
      assert.equal(context.manual, true);
      assert.deepEqual(context.draft, customPaths(client));
      assert.deepEqual(context.checked, customPaths(client));
      assert.equal(context.status.configPath, context.checked.configPath);
      assert.equal(context.status.skillPath, context.checked.skillPath);
      assert.notDeepEqual(context.checked, savedPaths[client]);
      assert.equal(session.canAct(action), true);
      session.confirm(action);
      assert.deepEqual(session.getSnapshot().pending, { action, client, locations: customPaths(client) });
      await session.run();
      assert.deepEqual(writes, [{ action, client, locations: customPaths(client) }]);
      assert.deepEqual(session.getSnapshot().contexts[other].checked, customPaths(other));
      assert.deepEqual(reads.at(-2), { client, locations: customPaths(client) });
      assert.deepEqual(reads.at(-1), { client: other, locations: customPaths(other) });
    }
  });

  test(`failed refresh cancels ${action} and cannot be bypassed by switching clients`, async () => {
    const { session, writes, overrideRead } = integrationFixture();
    await session.initialize();
    await checkManual(session, customPaths("codex"));
    session.confirm(action);
    let rejectRead;
    overrideRead(() => new Promise((_, reject) => { rejectRead = reject; }));
    const refresh = session.refresh();
    assert.equal(session.getSnapshot().busy, true);
    assert.equal(session.getSnapshot().pending, null);
    assert.equal(selectedContext(session).checked, null);
    assert.equal(session.canAct(action), false);
    session.confirm(action);
    await session.run();
    rejectRead(new Error("fixture read failed"));
    await refresh;
    session.select("claude_code");
    assert.equal(session.canAct(action), true);
    session.select("codex");
    assert.match(selectedContext(session).error, /fixture read failed/);
    assert.equal(selectedContext(session).manual, true);
    assert.deepEqual(selectedContext(session).draft, customPaths("codex"));
    assert.equal(session.canAct("configure"), false);
    assert.equal(session.canAct("remove"), false);
    session.confirm(action);
    await session.run();
    assert.deepEqual(writes, []);
    overrideRead(undefined);
    await session.refresh();
    session.confirm(action);
    await session.run();
    assert.deepEqual(writes, [{ action, client: "codex", locations: customPaths("codex") }]);
  });
}

test("switching or editing cancels confirmation and an unchecked draft remains blocked on return", async () => {
  const { session, writes } = integrationFixture();
  await session.initialize();
  await checkManual(session, customPaths("codex"));
  session.confirm("remove");
  session.select("claude_code");
  assert.equal(session.getSnapshot().pending, null);
  await session.run();
  session.select("codex");
  session.confirm("configure");
  session.editDraft("configPath", "C:/not-checked/config");
  assert.equal(session.getSnapshot().pending, null);
  session.select("deepseek_harness");
  session.select("codex");
  assert.equal(session.canAct("configure"), false);
  assert.equal(session.canAct("remove"), false);
  session.confirm("remove");
  await session.run();
  assert.deepEqual(writes, []);
});

test("returning to automatic paths requires a successful read and writes the exact displayed paths", async () => {
  const { session, savedPaths, writes, overrideRead } = integrationFixture();
  await session.initialize();
  await checkManual(session, customPaths("codex"));
  overrideRead(async () => { throw new Error("automatic read failed"); });
  await session.toggleManual();
  assert.equal(selectedContext(session).manual, false);
  assert.equal(session.canAct("configure"), false);
  assert.equal(session.canAct("remove"), false);
  session.select("claude_code"); session.select("codex");
  assert.match(selectedContext(session).error, /automatic read failed/);
  overrideRead(undefined);
  await session.refresh();
  const displayed = { configPath: selectedContext(session).status.configPath, skillPath: selectedContext(session).status.skillPath };
  session.confirm("configure");
  savedPaths.codex = { configPath: "C:/changed-default/config", skillPath: "C:/changed-default/skills/todolist-mcp" };
  await session.run();
  assert.deepEqual(writes, [{ action: "configure", client: "codex", locations: displayed }]);
});
