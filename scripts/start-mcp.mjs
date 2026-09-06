import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const executableName = process.platform === "win32" ? "todolist-mcp.exe" : "todolist-mcp";
const command = join(projectRoot, "target", "development", "release", executableName);
try {
  await access(command);
} catch {
  console.error("Development MCP is missing. Run npm run build:sidecar in the TodoList source project first.");
  process.exit(1);
}

const server = spawn(command, [], {
  cwd: projectRoot,
  env: process.env,
  stdio: "inherit",
});

server.on("error", (error) => {
  console.error(`Could not start TodoList MCP: ${error.message}`);
  process.exitCode = 1;
});

server.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 1;
});
