import { spawn } from "node:child_process";
import { access, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const tempDirectory = await mkdtemp(join(tmpdir(), "todolist-mcp-smoke-"));
const executableName = process.platform === "win32" ? "todolist-mcp.exe" : "todolist-mcp";
const releaseExecutable = join(process.cwd(), "target", "release", executableName);
let executableOverride = process.env.TODOLIST_MCP_EXECUTABLE;
if (!executableOverride) try {
  await access(releaseExecutable);
  executableOverride = releaseExecutable;
} catch {
  // The launcher will use a debug build or cargo when no release build exists.
}
const child = spawn(process.execPath, [join(process.cwd(), "scripts", "start-mcp.mjs")], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    TODOLIST_DB_PATH: join(tempDirectory, "todolist.sqlite"),
    ...(executableOverride ? { TODOLIST_MCP_EXECUTABLE: executableOverride } : {}),
  },
  stdio: ["pipe", "pipe", "pipe"],
});
console.log(`MCP smoke executable: ${executableOverride || "launcher fallback"}`);

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

try {
  send({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "todolist-smoke", version: "0.1.0" },
    },
  });
  const initialized = await waitFor(1);
  if (initialized.error) throw new Error(JSON.stringify(initialized.error));

  send({ jsonrpc: "2.0", method: "notifications/initialized", params: {} });
  send({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });
  const listed = await waitFor(2);
  if (listed.error) throw new Error(JSON.stringify(listed.error));

  const toolNames = listed.result.tools.map((tool) => tool.name).sort();
  const expected = ["create_project", "create_task", "get_task", "list_projects", "list_tasks", "update_task"];
  if (JSON.stringify(toolNames) !== JSON.stringify(expected)) {
    throw new Error(`Unexpected tools: ${toolNames.join(", ")}`);
  }
  const tools = new Map(listed.result.tools.map((tool) => [tool.name, tool]));
  for (const [toolName, propertyNames] of [
    ["create_project", ["request_id", "name", "color"]],
    ["create_task", ["request_id", "project_id", "title"]],
    ["list_tasks", ["limit", "cursor"]],
  ]) {
    const properties = tools.get(toolName)?.inputSchema?.properties ?? {};
    for (const propertyName of propertyNames) {
      if (!(propertyName in properties)) {
        throw new Error(`${toolName} schema is missing ${propertyName}`);
      }
    }
  }
  console.log(`MCP stdio smoke passed: ${toolNames.join(", ")}`);
} finally {
  child.stdin.end();
  child.kill();
  await rm(tempDirectory, { recursive: true, force: true });
}
