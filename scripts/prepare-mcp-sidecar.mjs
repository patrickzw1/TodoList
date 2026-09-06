import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const windows = process.platform === "win32";
const executableExtension = windows ? ".exe" : "";
const cargo = process.env.CARGO || (windows ? join(homedir(), ".cargo", "bin", "cargo.exe") : "cargo");
const rustc = process.env.RUSTC || (windows ? join(homedir(), ".cargo", "bin", "rustc.exe") : "rustc");
const production = process.argv.includes("--production");
const targetDirectory = process.env.CARGO_TARGET_DIR
  ? resolve(projectRoot, process.env.CARGO_TARGET_DIR)
  : join(projectRoot, "target", production ? "package-build" : "development");
const developmentDirectory = join(projectRoot, "target", "development");
const normalizedTarget = windows ? targetDirectory.toLowerCase() : targetDirectory;
const normalizedDevelopment = windows ? developmentDirectory.toLowerCase() : developmentDirectory;
if (production && (normalizedTarget === normalizedDevelopment || normalizedTarget.startsWith(normalizedDevelopment + sep))) {
  throw new Error("Production sidecars cannot be built into target/development. Use scripts/build-release.ps1 or a separate CARGO_TARGET_DIR.");
}

execFileSync(cargo, ["build", "--release", "--locked", "-p", "todolist-mcp", ...(production ? ["--features", "production"] : [])], {
  cwd: projectRoot,
  env: { ...process.env, CARGO_TARGET_DIR: targetDirectory },
  stdio: "inherit",
});

const targetTriple = execFileSync(rustc, ["--print", "host-tuple"], {
  cwd: projectRoot,
  encoding: "utf8",
}).trim();

if (!targetTriple) throw new Error("Rust did not report a host target triple");

const source = join(targetDirectory, process.env.CARGO_BUILD_TARGET || "", "release", `todolist-mcp${executableExtension}`);
const destinationDirectory = join(projectRoot, "src-tauri", "binaries");
const destination = join(destinationDirectory, `todolist-mcp-${targetTriple}${executableExtension}`);
mkdirSync(destinationDirectory, { recursive: true });
copyFileSync(source, destination);
console.log(`Prepared MCP sidecar: ${source} -> ${destination}`);
