import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const child = spawn(process.execPath, [join(projectRoot, "node_modules/@tauri-apps/cli/tauri.js"), ...process.argv.slice(2)], {
  cwd: projectRoot,
  env: { ...process.env, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || join(projectRoot, "target/development") },
  stdio: "inherit",
});
child.on("error", (error) => { console.error(error.message); process.exitCode = 1; });
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 1;
});
