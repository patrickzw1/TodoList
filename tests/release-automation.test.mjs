import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { verifyRelease } from "../scripts/verify-release.mjs";

function updaterFixture(content) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const rawPublicKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const keyId = randomBytes(8);
  const publicPacket = Buffer.concat([Buffer.from("Ed"), keyId, rawPublicKey]);
  const publicText = `untrusted comment: fixture key\n${publicPacket.toString("base64")}\n`;
  const digest = createHash("blake2b512").update(content).digest();
  const contentSignature = sign(null, digest, privateKey);
  const signaturePacket = Buffer.concat([Buffer.from("ED"), keyId, contentSignature]);
  const trustedPayload = "timestamp:1\tfile:fixture";
  const globalSignature = sign(null, Buffer.concat([contentSignature, Buffer.from(trustedPayload)]), privateKey);
  const signatureText = [
    "untrusted comment: fixture signature",
    signaturePacket.toString("base64"),
    `trusted comment: ${trustedPayload}`,
    globalSignature.toString("base64"),
  ].join("\n");
  return {
    encodedPublicKey: Buffer.from(publicText).toString("base64"),
    encodedSignature: Buffer.from(signatureText).toString("base64"),
  };
}

test("release verifier accepts matching signed assets and rejects tampering", async () => {
  const root = await mkdtemp(join(tmpdir(), "todolist-release-verifier-"));
  const version = "1.2.3";
  const tag = `v${version}`;
  const repository = "example/TodoList";
  const installerName = `TodoList_${version}_x64-setup.exe`;
  const bundle = join(root, "target/package-build/release/bundle/nsis");
  try {
    await mkdir(join(root, "src-tauri"), { recursive: true });
    await mkdir(join(root, "crates/task-core"), { recursive: true });
    await mkdir(join(root, "crates/task-store-sqlite"), { recursive: true });
    await mkdir(join(root, "crates/todolist-mcp"), { recursive: true });
    await mkdir(bundle, { recursive: true });
    await writeFile(join(root, "package.json"), JSON.stringify({ version }));
    await writeFile(join(root, "src-tauri/tauri.conf.json"), JSON.stringify({ version }));
    for (const path of ["src-tauri/Cargo.toml", "crates/task-core/Cargo.toml", "crates/task-store-sqlite/Cargo.toml", "crates/todolist-mcp/Cargo.toml"]) {
      await writeFile(join(root, path), `[package]\nname = "fixture"\nversion = "${version}"\n\n[dependencies]\n`);
    }

    const installer = Buffer.from("deterministic release verifier fixture");
    const signature = updaterFixture(installer);
    await writeFile(join(root, "src-tauri/tauri.release.conf.json"), JSON.stringify({ plugins: { updater: { pubkey: signature.encodedPublicKey } } }));
    await writeFile(join(bundle, installerName), installer);
    await writeFile(join(bundle, `${installerName}.sig`), signature.encodedSignature);
    const latest = {
      version,
      platforms: {
        "windows-x86_64": {
          signature: signature.encodedSignature,
          url: `https://github.com/${repository}/releases/download/${tag}/${installerName}`,
        },
      },
    };
    await writeFile(join(bundle, "latest.json"), JSON.stringify(latest));
    const checksums = [installerName, `${installerName}.sig`, "latest.json"].map(async (name) => {
      const content = await readFile(join(bundle, name));
      return `${createHash("sha256").update(content).digest("hex")}  ${name}`;
    });
    await writeFile(join(bundle, "SHA256SUMS.txt"), `${(await Promise.all(checksums)).join("\n")}\n`);

    const result = await verifyRelease({ root, tag, repository, verifyExecutables: false });
    assert.equal(result.signature, "verified");
    assert.equal(result.artifacts.length, 4);

    await writeFile(join(bundle, installerName), Buffer.from("tampered"));
    await assert.rejects(
      verifyRelease({ root, tag, repository, verifyExecutables: false }),
      /Updater artifact signature is invalid/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("CI and release workflows keep testing, signing and publication boundaries separate", async () => {
  const ci = await readFile(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8");
  const release = await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");
  assert.match(ci, /push:\s*\r?\n\s+branches:\s*\r?\n\s+- main/);
  assert.match(ci, /pull_request:/);
  assert.match(ci, /permissions:\s*\r?\n\s+contents: read/);
  assert.doesNotMatch(ci, /secrets\.|build-release|publish-github-release/);
  for (const command of ["typecheck", "test:app", "test:channels", "test:release-automation", "test:release-powershell", "test:sites", "cargo test --workspace --locked --no-fail-fast"]) {
    assert.match(ci, new RegExp(command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }

  assert.match(release, /tags:\s*\r?\n\s+- "v\*"/);
  assert.match(release, /workflow_dispatch:[\s\S]*?tag:[\s\S]*?required: true/);
  assert.match(release, /permissions:\s*\r?\n\s+contents: write/);
  assert.match(release, /cancel-in-progress: false/);
  assert.match(release, /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY \}\}/);
  assert.match(release, /TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD \}\}/);
  assert.ok(release.indexOf("verify-release.mjs") < release.indexOf("publish-github-release.ps1"));
  for (const command of ["typecheck", "test:app", "test:channels", "test:release-automation", "test:release-powershell", "test:sites", "cargo test --workspace --locked --no-fail-fast"]) {
    assert.match(release, new RegExp(command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
  for (const workflow of [ci, release]) {
    assert.match(workflow, /actions\/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7\.0\.1/);
    const sidecarPreparation = workflow.indexOf("run: npm run build:sidecar");
    const rustTests = workflow.indexOf("run: cargo test --workspace --locked --no-fail-fast");
    assert.ok(sidecarPreparation >= 0, "Workflow must prepare the real development MCP sidecar");
    assert.ok(sidecarPreparation < rustTests, "Development MCP sidecar must exist before desktop Rust tests");
    for (const match of workflow.matchAll(/uses:\s+[^@\s]+@([^\s]+)/g)) {
      assert.match(match[1], /^[0-9a-f]{40}$/, `Action is not pinned to a full commit: ${match[0]}`);
    }
  }
});

test("release scripts enforce the cloud signing and draft-release contract", async () => {
  const build = await readFile(new URL("../scripts/build-release.ps1", import.meta.url), "utf8");
  const validate = await readFile(new URL("../scripts/validate-release-ref.ps1", import.meta.url), "utf8");
  const publish = await readFile(new URL("../scripts/publish-github-release.ps1", import.meta.url), "utf8");
  assert.match(build, /GITHUB_ACTIONS/);
  assert.match(build, /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(build, /TAURI_SIGNING_PRIVATE_KEY_PASSWORD/);
  assert.match(validate, /merge-base --is-ancestor/);
  assert.match(validate, /origin\/main/);
  assert.match(publish, /Get-RemoteTagCommit/);
  assert.match(publish, /function Find-ReleaseByTag/);
  assert.match(publish, /releases\?per_page=100&page=\$page/);
  assert.match(publish, /function Get-ReleaseById/);
  assert.doesNotMatch(publish, /releases\/tags\//);
  assert.match(publish, /Remote \$Tag moved after checkout/);
  assert.match(publish, /-draft=false/);
  assert.match(publish, /release', 'download'/);
  assert.match(publish, /Get-Sha256/);
  assert.doesNotMatch(publish, /release', 'delete'/);
});
