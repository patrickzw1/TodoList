import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const root = await mkdtemp(join(tmpdir(), "todolist-multiclient-"));
const executable = join(root, process.platform === "win32" ? "todolist-mcp.exe" : "todolist-mcp");
const original = process.env.TODOLIST_MCP_EXECUTABLE ?? join(process.cwd(), "target/debug", "todolist-mcp.exe");
await copyFile(original, executable);
const databasePath = join(root, "fixture.sqlite");
const db = new DatabaseSync(databasePath);
db.exec("CREATE TABLE workspace_snapshot (id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL, payload_json TEXT NOT NULL, updated_at INTEGER NOT NULL)");
const fixture = { version: 1, projects: [{ id: "p", name: "Fixture", color: "#1264f4" }], tasks: [] };
db.prepare("INSERT INTO workspace_snapshot VALUES(1,?,?,0)").run(1, JSON.stringify(fixture)); db.close();
const file = join(root, "source.pdf"); await writeFile(file, "source remains unchanged");
const clients = [];
function start(client) {
  const process = spawn(executable, client ? ["--client", client] : [], { cwd: root, env: { ...globalThis.process.env, TODOLIST_DB_PATH: databasePath }, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  const pending = new Map(); let nextId = 0, buffer = "", stderr = "";
  process.stdout.setEncoding("utf8"); process.stderr.setEncoding("utf8");
  process.stderr.on("data", (data) => { stderr += data; });
  process.stdout.on("data", (data) => {
    buffer += data; const lines = buffer.split(/\r?\n/); buffer = lines.pop();
    for (const line of lines) {
      if (!line.trim()) continue;
      const message = JSON.parse(line); const callback = pending.get(message.id);
      if (callback) { pending.delete(message.id); callback(message); }
    }
  });
  const exited = once(process, "exit");
  function request(method, params) {
    const id = ++nextId;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`${client ?? "generic"}: timeout ${method}: ${stderr}`)); }, 15_000);
      pending.set(id, (result) => { clearTimeout(timeout); result.error ? reject(new Error(JSON.stringify(result.error))) : resolve(result.result); });
      process.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
    });
  }
  const connection = { process, exited, request, client,
    call: (name, args) => request("tools/call", { name, arguments: args }),
    async close() { process.stdin.end(); const timeout = setTimeout(() => process.kill(), 3000); await exited; clearTimeout(timeout); },
  };
  clients.push(connection); return connection;
}
function decoded(result) { assert.ok(!result.isError, result.content?.[0]?.text); const data = JSON.parse(result.content[0].text); if (data.task) assert.ok(!("activity" in data.task)); return data; }
async function initialize(client, protocolVersion) {
  // Deliberately misleading clientInfo: origins must still come only from --client.
  const result = await client.request("initialize", { protocolVersion, capabilities: {}, clientInfo: { name: "model_claims_to_be_codex", version: "1" } });
  client.process.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");
  const tools = await client.request("tools/list", {});
  assert.equal(tools.tools.length, 10); assert.ok(tools.tools.some((t) => t.name === "get_task_activity"));
  console.log(`${client.client ?? "generic"}: protocol ${result.protocolVersion}, 10 tools`);
}
try {
  const codex = start("codex"), cc = start("claude_code"), dsh = start("deepseek_harness"), generic = start();
  await Promise.all([initialize(codex, "2025-06-18"), initialize(cc, "2025-06-18"), initialize(dsh, "2026-07-28"), initialize(generic, "2025-06-18")]);
  const created = await Promise.all([codex, cc, dsh, generic].map(async (client) => {
    const input = { project_id: "p", title: `${client.client ?? "generic"} task`, request_id: "same-request-id" };
    const task = decoded(await client.call("create_task", input)).task;
    const expectedLabel = { codex: "Codex", claude_code: "Claude Code", deepseek_harness: "DeepSeek Harness" }[client.client] ?? "AI";
    assert.equal(task.source, `${expectedLabel} 创建`); assert.equal(task.pinned, false);
    const replay = decoded(await client.call("create_task", input)); assert.equal(replay.task.id, task.id); assert.equal(replay.replayed, true);
    const history = decoded(await client.call("get_task_activity", { task_id: task.id })); assert.equal(history.activity[0].actor, client.client ?? "ai");
    return task;
  }));
  assert.equal(new Set(created.map((t) => t.id)).size, 4, "retry ids are scoped to configured clients");
  const list = decoded(await codex.call("list_tasks", { project_id: "p", limit: 1 })); assert.ok(list.nextCursor);
  const task = created[0];
  const race = await Promise.all([cc.call("update_task", { task_id: task.id, expected_version: task.version, description: "CC wrote" }), dsh.call("update_task", { task_id: task.id, expected_version: task.version, description: "DSH wrote" })]);
  assert.equal(race.filter((r) => !r.isError).length, 1); assert.equal(race.filter((r) => r.isError && r.content[0].text.includes("version_conflict")).length, 1);
  assert.ok((await codex.call("list_tasks", { project_id: "p", cursor: list.nextCursor, limit: 1 })).isError);
  const current = decoded(await dsh.call("get_task", { task_id: task.id })).task;
  const attachInput = { task_id: task.id, expected_version: current.version, source_path: file, request_id: "one-attachment" };
  const attached = decoded(await dsh.call("add_task_attachment", attachInput)); assert.equal(attached.task.attachments.length, 1);
  const retry = decoded(await dsh.call("add_task_attachment", attachInput)); assert.equal(retry.file.id, attached.file.id); assert.equal(retry.replayed, true);
  assert.equal((await readFile(file, "utf8")), "source remains unchanged");
  const history = decoded(await cc.call("get_task_activity", { task_id: task.id, limit: 1 })); assert.equal(history.activity[0].actor, "deepseek_harness"); assert.ok(history.nextCursor);
  const latest = decoded(await cc.call("get_task", { task_id: task.id })).task;
  decoded(await cc.call("update_task", { task_id: task.id, expected_version: latest.version, tags: ["preserve source"] }));
  assert.ok((await cc.call("get_task_activity", { task_id: task.id, limit: 1, cursor: history.nextCursor })).isError);
  await dsh.close(); const restarted = start("deepseek_harness"); await initialize(restarted, "2026-07-28");
  const persisted = decoded(await restarted.call("get_task", { task_id: task.id })).task;
  assert.equal(persisted.source, "Codex 创建"); assert.equal(persisted.attachments.length, 1); assert.deepEqual(persisted.tags, ["preserve source"]);
  console.log("PASS: independent processes, shared isolated DB, client origins, scoped retry ids, atomic version conflict, stale cursors, attachment retry, restart persistence");
} finally {
  await Promise.allSettled(clients.filter((c) => c.process.exitCode === null).map((c) => c.close()));
  await rm(root, { recursive: true, force: true });
}
