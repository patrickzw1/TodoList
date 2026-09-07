import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const tempDirectory = await mkdtemp(join(tmpdir(), "todolist-mcp-smoke-"));
const databasePath = join(tempDirectory, "todolist.sqlite");
const appVersion = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8")).version;
const executableName = process.platform === "win32" ? "todolist-mcp.exe" : "todolist-mcp";
const executable = process.env.TODOLIST_MCP_EXECUTABLE || join(process.cwd(), "target", "development", "release", executableName);
const task = (id, projectId, archived = false) => ({
  id, projectId, title: id, description: "", status: "todo", priority: "medium",
  dueLabel: "未安排", dueDate: "9999-12-31", tags: [], source: "smoke", archived,
  pinned: false, version: 1, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [],
});
const fixture = {
  version: 1,
  projects: [
    { id: "project-1", name: "One", color: "#1264f4" },
    { id: "project-2", name: "Two", color: "#258ca6" },
  ],
  tasks: [task("task-a", "project-1"), task("task-x", "project-2"), task("task-b", "project-1"), task("task-z", "project-1", true)],
};
const database = new DatabaseSync(databasePath);
database.exec("CREATE TABLE workspace_snapshot (id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL, payload_json TEXT NOT NULL, updated_at INTEGER NOT NULL)");
database.prepare("INSERT INTO workspace_snapshot (id, version, payload_json, updated_at) VALUES (1, ?, ?, ?)")
  .run(fixture.version, JSON.stringify(fixture), Math.floor(Date.now() / 1000));
database.close();

const child = spawn(executable, [], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    TODOLIST_DB_PATH: databasePath,
  },
  stdio: ["pipe", "pipe", "pipe"],
});
console.log(`MCP smoke executable: ${executable}`);

const responses = new Map();
let stderr = "";
let output = "";

child.stderr.setEncoding("utf8");
child.stderr.on("data", (chunk) => { stderr += chunk; });
child.stdout.setEncoding("utf8");
child.stdout.on("data", (chunk) => {
  output += chunk;
  const lines = output.split(/\r?\n/);
  output = lines.pop() ?? "";
  for (const line of lines) {
    if (!line.trim()) continue;
    const message = JSON.parse(line);
    if (message.id !== undefined) responses.set(message.id, message);
  }
});

function send(message) {
  child.stdin.write(`${JSON.stringify(message)}\n`);
}

async function waitFor(id) {
  const deadline = Date.now() + 10_000;
  while (!responses.has(id)) {
    if (Date.now() > deadline) throw new Error(`Timed out waiting for response ${id}. stderr: ${stderr}`);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  return responses.get(id);
}

let requestId = 2;
async function callTool(name, args) {
  const id = ++requestId;
  send({ jsonrpc: "2.0", id, method: "tools/call", params: { name, arguments: args } });
  const response = await waitFor(id);
  if (response.error) throw new Error(JSON.stringify(response.error));
  return response.result;
}

function toolJson(result) {
  if (result.isError) throw new Error(result.content?.[0]?.text || "Tool call failed");
  return JSON.parse(result.content[0].text);
}

try {
  send({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "todolist-smoke", version: appVersion },
    },
  });
  const initialized = await waitFor(1);
  if (initialized.error) throw new Error(JSON.stringify(initialized.error));
  for (const capability of ["attachments", "images", "desktop UI", "no file upload"]) {
    if (!initialized.result.instructions?.includes(capability)) throw new Error(`Missing MCP capability guidance: ${capability}`);
  }

  send({ jsonrpc: "2.0", method: "notifications/initialized", params: {} });
  send({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });
  const listed = await waitFor(2);
  if (listed.error) throw new Error(JSON.stringify(listed.error));

  const toolNames = listed.result.tools.map((tool) => tool.name).sort();
  const expected = ["create_project", "create_task", "get_task", "list_projects", "list_tasks", "reorder_tasks", "update_task"];
  if (JSON.stringify(toolNames) !== JSON.stringify(expected)) {
    throw new Error(`Unexpected tools: ${toolNames.join(", ")}`);
  }
  const tools = new Map(listed.result.tools.map((tool) => [tool.name, tool]));
  if (!tools.get("get_task")?.description?.includes("attachments/images")) throw new Error("File metadata is not documented in get_task");
  for (const [toolName, propertyNames] of [
    ["create_project", ["request_id", "name", "color"]],
    ["create_task", ["request_id", "project_id", "title"]],
    ["list_tasks", ["limit", "cursor"]],
    ["reorder_tasks", ["project_id", "archived", "expected_workspace_version", "task_ids"]],
  ]) {
    const properties = tools.get(toolName)?.inputSchema?.properties ?? {};
    for (const propertyName of propertyNames) {
      if (!(propertyName in properties)) {
        throw new Error(`${toolName} schema is missing ${propertyName}`);
      }
    }
  }

  const firstPage = toolJson(await callTool("list_tasks", { project_id: "project-1", limit: 1 }));
  const initial = toolJson(await callTool("list_tasks", { project_id: "project-1" }));
  if (initial.tasks.map((item) => item.id).join(",") !== "task-a,task-b") throw new Error("Unexpected initial task order");
  const reordered = toolJson(await callTool("reorder_tasks", {
    project_id: "project-1", archived: false, expected_workspace_version: initial.workspaceVersion, task_ids: ["task-b", "task-a"],
  }));
  if (!reordered.changed || reordered.workspaceVersion !== initial.workspaceVersion + 1) throw new Error("Real reorder did not advance the workspace once");
  const after = toolJson(await callTool("list_tasks", { project_id: "project-1" }));
  if (after.tasks.map((item) => item.id).join(",") !== "task-b,task-a") throw new Error("Reordered list_tasks result is inconsistent");

  const stalePage = await callTool("list_tasks", { project_id: "project-1", limit: 1, cursor: firstPage.nextCursor });
  if (!stalePage.isError || !stalePage.content[0].text.includes("stale")) throw new Error("Old list cursor was not rejected after reorder");
  for (const [label, task_ids] of [
    ["stale workspace", ["task-a", "task-b"]],
    ["duplicate", ["task-b", "task-b"]],
    ["missing", ["task-b"]],
    ["cross project", ["task-b", "task-x"]],
    ["cross archived", ["task-b", "task-z"]],
  ]) {
    const failed = await callTool("reorder_tasks", {
      project_id: "project-1", archived: false,
      expected_workspace_version: label === "stale workspace" ? initial.workspaceVersion : after.workspaceVersion,
      task_ids,
    });
    if (!failed.isError) throw new Error(`${label} reorder should fail`);
  }
  const unchanged = toolJson(await callTool("list_tasks", { project_id: "project-1" }));
  if (unchanged.workspaceVersion !== after.workspaceVersion || unchanged.tasks.map((item) => item.id).join(",") !== "task-b,task-a") {
    throw new Error("Failed reorder partially changed the real temporary database");
  }
  console.log(`MCP stdio smoke passed: ${toolNames.join(", ")}`);
} finally {
  child.stdin.end();
  child.kill();
  await rm(tempDirectory, { recursive: true, force: true });
}
