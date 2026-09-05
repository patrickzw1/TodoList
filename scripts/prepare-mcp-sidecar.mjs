import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const windows = process.platform === "win32";
const executableExtension = windows ? ".exe" : "";
const cargo = process.env.CARGO || (windows ? join(homedir(), ".cargo", "bin", "cargo.exe") : "cargo");
const rustc = process.env.RUSTC || (windows ? join(homedir(), ".cargo", "bin", "rustc.exe") : "rustc");

execFileSync(cargo, ["build", "--release", "--locked", "-p", "todolist-mcp"], {
  cwd: projectRoot,
  stdio: "inherit",
});

const targetTriple = execFileSync(rustc, ["--print", "host-tuple"], {
  cwd: projectRoot,
  encoding: "utf8",
}).trim();

if (!targetTriple) throw new Error("Rust did not report a host target triple");

const targetDirectory = process.env.CARGO_TARGET_DIR
  ? resolve(projectRoot, process.env.CARGO_TARGET_DIR)
  : join(projectRoot, "target");
const source = join(targetDirectory, process.env.CARGO_BUILD_TARGET || "", "release", `todolist-mcp${executableExtension}`);
const destinationDirectory = join(projectRoot, "src-tauri", "binaries");
const destination = join(destinationDirectory, `todolist-mcp-${targetTriple}${executableExtension}`);
mkdirSync(destinationDirectory, { recursive: true });
copyFileSync(source, destination);
console.log(`Prepared MCP sidecar: ${source} -> ${destination}`);
