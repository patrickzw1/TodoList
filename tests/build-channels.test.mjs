import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("development and production packaging select matching application and sidecar channels", async () => {
  const development = JSON.parse(await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url)));
  const production = JSON.parse(await readFile(new URL("../src-tauri/tauri.release.conf.json", import.meta.url)));
  const example = JSON.parse(await readFile(new URL("../src-tauri/tauri.release.conf.example.json", import.meta.url)));
  assert.equal(development.identifier, "app.todolist.desktop.dev");
  assert.equal(development.productName, "TodoList Dev");
  assert.equal(development.build.features?.includes("production") ?? false, false);
  assert.doesNotMatch(development.build.beforeBuildCommand, /--production/);
  assert.deepEqual(development.plugins.updater.endpoints, []);
  for (const release of [production, example]) {
    assert.equal(release.identifier, "app.todolist.desktop");
    assert.equal(release.productName, "TodoList");
    assert.ok(release.build.features.includes("production"));
    assert.match(release.build.beforeBuildCommand, /build:sidecar -- --production/);
    assert.match(release.build.beforeDevCommand, /build:sidecar -- --production/);
  }
  const projectConfig = await readFile(new URL("../.codex/config.toml", import.meta.url), "utf8");
  assert.match(projectConfig, /\[mcp_servers\.todolist_dev\]/);
  assert.doesNotMatch(projectConfig, /\[mcp_servers\.todolist\]/);
});

test("production preparation cannot replace the development MCP binary", () => {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const result = spawnSync(process.execPath, [join(root, "scripts/prepare-mcp-sidecar.mjs"), "--production"], {
    cwd: root,
    env: { ...process.env, CARGO_TARGET_DIR: join(root, "target/development") },
    encoding: "utf8",
    timeout: 10_000,
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Production sidecars cannot be built into target\/development/);
});

test("development launcher refuses legacy executables and inherited executable overrides", async () => {
  const root = await mkdtemp(join(tmpdir(), "todolist-launcher-test-"));
  try {
    await mkdir(join(root, "scripts"));
    await mkdir(join(root, "target/release"), { recursive: true });
    await mkdir(join(root, "target/debug"), { recursive: true });
    const executableName = process.platform === "win32" ? "todolist-mcp.exe" : "todolist-mcp";
    for (const profile of ["debug", "release"]) {
      await writeFile(join(root, "target", profile, executableName), "legacy build must not run");
    }
    await copyFile(new URL("../scripts/start-mcp.mjs", import.meta.url), join(root, "scripts/start-mcp.mjs"));
    const result = spawnSync(process.execPath, [join(root, "scripts/start-mcp.mjs")], {
      cwd: root,
      env: { ...process.env, TODOLIST_MCP_EXECUTABLE: process.execPath },
      encoding: "utf8",
      timeout: 10_000,
    });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Development MCP is missing/);
    assert.equal(result.stdout, "");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
