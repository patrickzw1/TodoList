import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

const STABLE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const CARGO_MANIFESTS = [
  ["src-tauri/Cargo.toml", "todolist-desktop"],
  ["crates/task-core/Cargo.toml", "task-core"],
  ["crates/task-diagnostics/Cargo.toml", "task-diagnostics"],
  ["crates/task-store-sqlite/Cargo.toml", "task-store-sqlite"],
  ["crates/todolist-mcp/Cargo.toml", "todolist-mcp"],
];

function replaceSingle(text, pattern, replacement, label) {
  const matches = [...text.matchAll(pattern)];
  if (matches.length !== 1) throw new Error(`${label} must contain exactly one version field`);
  return text.replace(pattern, replacement);
}

function packageBlock(text, header, label) {
  const start = text.indexOf(header);
  if (start < 0) throw new Error(`${label} is missing ${header}`);
  const contentStart = text.indexOf("\n", start) + 1;
  const nextSection = text.slice(contentStart).search(/^\[/m);
  const end = nextSection < 0 ? text.length : contentStart + nextSection;
  return { start, end, text: text.slice(start, end) };
}

function updateCargoManifest(text, expectedName, currentVersion, nextVersion, label) {
  const block = packageBlock(text, "[package]", label);
  if (!new RegExp(`^name\\s*=\\s*"${expectedName}"\\s*$`, "m").test(block.text)) {
    throw new Error(`${label} package name is not ${expectedName}`);
  }
  if (!new RegExp(`^version\\s*=\\s*"${currentVersion.replaceAll(".", "\\.")}"\\s*$`, "m").test(block.text)) {
    throw new Error(`${label} package version does not match package.json`);
  }
  const updatedBlock = replaceSingle(
    block.text,
    /^(version\s*=\s*")([^"]+)("\s*)$/gm,
    `$1${nextVersion}$3`,
    `${label} [package]`,
  );
  return text.slice(0, block.start) + updatedBlock + text.slice(block.end);
}

function cargoLockBlocks(text) {
  const starts = [...text.matchAll(/^\[\[package\]\]\s*$/gm)].map((match) => match.index);
  return starts.map((start, index) => ({
    start,
    end: index + 1 < starts.length ? starts[index + 1] : text.length,
    text: text.slice(start, index + 1 < starts.length ? starts[index + 1] : text.length),
  }));
}

function updateLocalCargoLockPackages(text, currentVersion, nextVersion) {
  const localNames = new Set(CARGO_MANIFESTS.map(([, name]) => name));
  const edits = [];
  for (const block of cargoLockBlocks(text)) {
    const name = block.text.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    if (!localNames.has(name)) continue;
    if (!new RegExp(`^version\\s*=\\s*"${currentVersion.replaceAll(".", "\\.")}"\\s*$`, "m").test(block.text)) {
      throw new Error(`Cargo.lock package ${name} does not match package.json`);
    }
    const updated = replaceSingle(
      block.text,
      /^(version\s*=\s*")([^"]+)("\s*)$/gm,
      `$1${nextVersion}$3`,
      `Cargo.lock package ${name}`,
    );
    edits.push({ ...block, updated, name });
  }
  if (edits.length !== localNames.size || new Set(edits.map((edit) => edit.name)).size !== localNames.size) {
    throw new Error("Cargo.lock must contain each local package exactly once");
  }
  for (const edit of edits.sort((left, right) => right.start - left.start)) {
    text = text.slice(0, edit.start) + edit.updated + text.slice(edit.end);
  }
  return text;
}

function updatePackageLock(text, currentVersion, nextVersion) {
  const parsed = JSON.parse(text);
  if (parsed.version !== currentVersion || parsed.packages?.[""]?.version !== currentVersion) {
    throw new Error("package-lock.json versions do not match package.json");
  }
  text = replaceSingle(
    text,
    /^(  "version"\s*:\s*")([^"]+)(",?\s*)$/gm,
    `$1${nextVersion}$3`,
    "package-lock.json root",
  );
  const rootHeader = "    \"\": {";
  const rootStart = text.indexOf(rootHeader);
  if (rootStart < 0) throw new Error("package-lock.json is missing its root package");
  const rootContentStart = text.indexOf("\n", rootStart) + 1;
  const nextPackage = text.slice(rootContentStart).search(/^    "[^"]+": \{$/m);
  if (nextPackage < 0) throw new Error("package-lock.json root package has no boundary");
  const rootPackage = {
    start: rootStart,
    end: rootContentStart + nextPackage,
    text: text.slice(rootStart, rootContentStart + nextPackage),
  };
  const updatedRootPackage = replaceSingle(
    rootPackage.text,
    /^(      "version"\s*:\s*")([^"]+)(",?\s*)$/gm,
    `$1${nextVersion}$3`,
    "package-lock.json root package",
  );
  return text.slice(0, rootPackage.start) + updatedRootPackage + text.slice(rootPackage.end);
}

function updateMcpHandler(text, currentVersion, nextVersion) {
  const start = text.indexOf("#[tool_handler(");
  const endMatch = start < 0 ? null : text.slice(start).match(/\)\]\r?\nimpl ServerHandler/);
  const end = endMatch ? start + endMatch.index : -1;
  if (start < 0 || end < 0) throw new Error("crates/todolist-mcp/src/lib.rs is missing its tool handler block");
  const block = { start, end, text: text.slice(start, end) };
  if (!new RegExp(`^\\s*version\\s*=\\s*"${currentVersion.replaceAll(".", "\\.")}",\\s*$`, "m").test(block.text)) {
    throw new Error("MCP server version does not match package.json");
  }
  const updated = replaceSingle(
    block.text,
    /^(\s*version\s*=\s*")([^"]+)(",\s*)$/gm,
    `$1${nextVersion}$3`,
    "MCP server handler",
  );
  return text.slice(0, block.start) + updated + text.slice(block.end);
}

export async function syncVersion({ root, version }) {
  if (!STABLE_VERSION.test(version)) throw new Error("Version must be a stable MAJOR.MINOR.PATCH value");
  const paths = {
    packageJson: join(root, "package.json"),
    packageLock: join(root, "package-lock.json"),
    tauriConfig: join(root, "src-tauri", "tauri.conf.json"),
    cargoLock: join(root, "Cargo.lock"),
    mcpServer: join(root, "crates", "todolist-mcp", "src", "lib.rs"),
  };
  const packageJsonText = await readFile(paths.packageJson, "utf8");
  const currentVersion = JSON.parse(packageJsonText).version;
  if (!STABLE_VERSION.test(currentVersion)) throw new Error("package.json does not contain a stable version");

  const contents = new Map(await Promise.all([
    ...Object.values(paths).filter((path) => path !== paths.packageJson).map(async (path) => [path, await readFile(path, "utf8")]),
    ...CARGO_MANIFESTS.map(async ([relative]) => [join(root, relative), await readFile(join(root, relative), "utf8")]),
  ]));
  const tauriVersion = JSON.parse(contents.get(paths.tauriConfig)).version;
  if (tauriVersion !== currentVersion) throw new Error("src-tauri/tauri.conf.json version does not match package.json");

  const updates = new Map();
  updates.set(paths.packageJson, replaceSingle(
    packageJsonText,
    /^(  "version"\s*:\s*")([^"]+)(",?\s*)$/gm,
    `$1${version}$3`,
    "package.json",
  ));
  updates.set(paths.packageLock, updatePackageLock(contents.get(paths.packageLock), currentVersion, version));
  updates.set(paths.tauriConfig, replaceSingle(
    contents.get(paths.tauriConfig),
    /^(  "version"\s*:\s*")([^"]+)(",?\s*)$/gm,
    `$1${version}$3`,
    "src-tauri/tauri.conf.json",
  ));
  for (const [relative, name] of CARGO_MANIFESTS) {
    const path = join(root, relative);
    updates.set(path, updateCargoManifest(contents.get(path), name, currentVersion, version, relative));
  }
  updates.set(paths.cargoLock, updateLocalCargoLockPackages(contents.get(paths.cargoLock), currentVersion, version));
  updates.set(paths.mcpServer, updateMcpHandler(contents.get(paths.mcpServer), currentVersion, version));

  await Promise.all([...updates].map(async ([path, content]) => writeFile(path, content, "utf8")));
  return { previousVersion: currentVersion, version, changedFiles: [...updates.keys()] };
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  const version = process.argv[2];
  if (!version) throw new Error("Usage: npm run sync:version -- MAJOR.MINOR.PATCH");
  const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
  const result = await syncVersion({ root, version });
  console.log(`Synchronized TodoList ${result.previousVersion} -> ${result.version} across ${result.changedFiles.length} owned files.`);
}
