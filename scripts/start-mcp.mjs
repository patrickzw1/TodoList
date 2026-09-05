import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

const executableName = process.platform === "win32" ? "todolist-mcp.exe" : "todolist-mcp";
const candidates = [
  process.env.TODOLIST_MCP_EXECUTABLE,
  join(process.cwd(), "target", "debug", executableName),
  join(process.cwd(), "target", "release", executableName),
].filter(Boolean);

let command;
let args = [];
for (const candidate of candidates) {
  try {
    await access(candidate);
    command = candidate;
    break;
  } catch {
    // Continue to the next local build candidate.
  }
}

if (!command) {
  const cargoName = process.platform === "win32" ? "cargo.exe" : "cargo";
  command = join(homedir(), ".cargo", "bin", cargoName);
  args = ["run", "--quiet", "-p", "todolist-mcp", "--"];
}

const server = spawn(command, args, {
  cwd: process.cwd(),
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
