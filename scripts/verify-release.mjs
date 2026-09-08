import assert from "node:assert/strict";
import { createHash, createPublicKey, verify as verifySignature } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const SPKI_ED25519_PREFIX = Buffer.from("302a300506032b6570032100", "hex");
const TRUSTED_COMMENT_PREFIX = "trusted comment: ";

function parseArguments(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`Invalid argument: ${name ?? ""}`);
    values[name.slice(2)] = value;
  }
  return values;
}

function decodeBase64(value, label) {
  assert.match(value, /^[A-Za-z0-9+/]+={0,2}$/, `${label} is not canonical base64`);
  const decoded = Buffer.from(value, "base64");
  assert.equal(decoded.toString("base64"), value, `${label} is not canonical base64`);
  return decoded;
}

function parsePublicKey(encodedPublicKey) {
  const publicText = decodeBase64(encodedPublicKey.trim(), "Updater public key").toString("utf8").trim();
  const lines = publicText.split(/\r?\n/);
  assert.equal(lines.length, 2, "Updater public key must contain exactly two lines");
  assert.match(lines[0], /^untrusted comment: /, "Updater public key comment is missing");
  const packet = decodeBase64(lines[1], "Updater public key packet");
  assert.equal(packet.length, 42, "Updater public key packet has an unexpected length");
  assert.equal(packet.subarray(0, 2).toString("ascii"), "Ed", "Updater public key algorithm is unsupported");
  return {
    id: packet.subarray(2, 10),
    key: createPublicKey({ key: Buffer.concat([SPKI_ED25519_PREFIX, packet.subarray(10)]), format: "der", type: "spki" }),
  };
}

export function verifyUpdaterSignature(content, encodedSignature, encodedPublicKey) {
  const publicKey = parsePublicKey(encodedPublicKey);
  const signatureText = decodeBase64(encodedSignature.trim(), "Updater signature envelope").toString("utf8").trim();
  const lines = signatureText.split(/\r?\n/);
  assert.equal(lines.length, 4, "Updater signature must contain exactly four lines");
  assert.match(lines[0], /^untrusted comment: /, "Updater signature comment is missing");
  assert.ok(lines[2].startsWith(TRUSTED_COMMENT_PREFIX), "Updater trusted comment is missing");

  const packet = decodeBase64(lines[1], "Updater signature packet");
  assert.equal(packet.length, 74, "Updater signature packet has an unexpected length");
  assert.deepEqual(packet.subarray(2, 10), publicKey.id, "Updater signature uses a different key");
  const algorithm = packet.subarray(0, 2).toString("ascii");
  assert.ok(algorithm === "ED" || algorithm === "Ed", `Updater signature algorithm is unsupported: ${algorithm}`);
  const signedContent = algorithm === "ED" ? createHash("blake2b512").update(content).digest() : content;
  assert.ok(verifySignature(null, signedContent, publicKey.key, packet.subarray(10)), "Updater artifact signature is invalid");

  const globalSignature = decodeBase64(lines[3], "Updater trusted-comment signature");
  assert.equal(globalSignature.length, 64, "Updater trusted-comment signature has an unexpected length");
  const trustedPayload = Buffer.from(lines[2].slice(TRUSTED_COMMENT_PREFIX.length), "utf8");
  assert.ok(
    verifySignature(null, Buffer.concat([packet.subarray(10), trustedPayload]), publicKey.key, globalSignature),
    "Updater trusted-comment signature is invalid",
  );
}

function readPeSubsystem(content, label) {
  assert.equal(content.subarray(0, 2).toString("ascii"), "MZ", `${label} is not a Windows PE file`);
  const peOffset = content.readUInt32LE(0x3c);
  assert.ok(peOffset + 94 <= content.length, `${label} has a truncated PE header`);
  assert.equal(content.subarray(peOffset, peOffset + 4).toString("binary"), "PE\0\0", `${label} has an invalid PE header`);
  return content.readUInt16LE(peOffset + 24 + 68);
}

function assertNoPrivateBuildPath(content, label) {
  const decoded = [content.toString("latin1"), content.toString("utf16le")];
  const patterns = [
    /[A-Za-z]:[\\/]+Users[\\/]+[^\\/\0\r\n]+/i,
    /[A-Za-z]:[\\/]+AppDevelopment[\\/]+todo/i,
    /[A-Za-z]:[\\/]+[^\0\r\n]*?\.codex[\\/]+worktrees/i,
  ];
  for (const text of decoded) {
    for (const pattern of patterns) assert.doesNotMatch(text, pattern, `Private build path remains in ${label}`);
  }
}

function runIdentityChecks(desktopPath, mcpPath, version) {
  assert.equal(process.platform, "win32", "Executable identity checks require Windows");
  const versionProbe = spawnSync(
    "powershell.exe",
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "[Console]::Out.Write((Get-Item -LiteralPath $env:TODOLIST_RELEASE_DESKTOP).VersionInfo.ProductVersion)"],
    { env: { ...process.env, TODOLIST_RELEASE_DESKTOP: desktopPath }, encoding: "utf8", windowsHide: true, timeout: 10_000 },
  );
  if (versionProbe.error) throw new Error(`Desktop version probe could not start: ${versionProbe.error.message}`);
  assert.equal(versionProbe.status, 0, `Desktop version probe failed: ${(versionProbe.stderr ?? "").trim()}`);
  assert.equal(versionProbe.stdout.trim(), version, "Desktop product version does not match package.json");

  const identityProbe = spawnSync(mcpPath, ["--print-build-identity"], { encoding: "utf8", windowsHide: true, timeout: 10_000 });
  if (identityProbe.error) throw new Error(`MCP identity probe could not start: ${identityProbe.error.message}`);
  assert.equal(identityProbe.status, 0, `MCP identity probe failed: ${(identityProbe.stderr ?? "").trim()}`);
  assert.equal(identityProbe.stdout.trim(), `todolist/${version}/production`, "MCP build identity is not the matching production build");
}

function packageVersionFromCargoToml(text, label) {
  const packageStart = text.search(/^\[package\]\s*$/m);
  assert.ok(packageStart >= 0, `${label} has no package section`);
  const headerEnd = text.indexOf("\n", packageStart);
  assert.ok(headerEnd >= 0, `${label} has an empty package section`);
  const remainder = text.slice(headerEnd + 1);
  const nextSection = remainder.search(/^\[/m);
  const packageBlock = nextSection >= 0 ? remainder.slice(0, nextSection) : remainder;
  const version = packageBlock.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  assert.ok(version, `${label} has no package version`);
  return version;
}

export async function verifyRelease({ root, tag, repository, verifyExecutables = true }) {
  const packageJson = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
  const version = packageJson.version;
  assert.match(version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/, "package.json version must be a stable semantic version");
  assert.equal(tag, `v${version}`, "Release tag does not match package.json version");
  assert.match(repository, /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/, "Repository must use owner/name format");

  const developmentConfig = JSON.parse(await readFile(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
  const releaseConfig = JSON.parse(await readFile(join(root, "src-tauri", "tauri.release.conf.json"), "utf8"));
  assert.equal(developmentConfig.version, version, "Tauri version does not match package.json");
  for (const cargoToml of ["src-tauri/Cargo.toml", "crates/task-core/Cargo.toml", "crates/task-store-sqlite/Cargo.toml", "crates/todolist-mcp/Cargo.toml"]) {
    const cargoVersion = packageVersionFromCargoToml(await readFile(join(root, cargoToml), "utf8"), cargoToml);
    assert.equal(cargoVersion, version, `${cargoToml} version does not match package.json`);
  }

  const bundleDirectory = join(root, "target", "package-build", "release", "bundle", "nsis");
  const installerName = `TodoList_${version}_x64-setup.exe`;
  const expectedChecksumNames = [installerName, `${installerName}.sig`, "latest.json"];
  const publicAssetNames = [...expectedChecksumNames, "SHA256SUMS.txt"];
  const paths = Object.fromEntries(publicAssetNames.map((name) => [name, join(bundleDirectory, name)]));
  const contents = Object.fromEntries(await Promise.all(publicAssetNames.map(async (name) => [name, await readFile(paths[name])])));

  const encodedSignature = contents[`${installerName}.sig`].toString("utf8").trim();
  verifyUpdaterSignature(contents[installerName], encodedSignature, releaseConfig.plugins.updater.pubkey);

  const manifest = JSON.parse(contents["latest.json"].toString("utf8"));
  const platform = manifest.platforms?.["windows-x86_64"];
  assert.equal(manifest.version, version, "latest.json version does not match package.json");
  assert.equal(platform?.signature, encodedSignature, "latest.json signature does not match the signature asset");
  assert.equal(
    platform?.url,
    `https://github.com/${repository}/releases/download/${tag}/${installerName}`,
    "latest.json download URL does not match this repository and tag",
  );

  const checksumLines = contents["SHA256SUMS.txt"].toString("utf8").trim().split(/\r?\n/);
  assert.equal(checksumLines.length, expectedChecksumNames.length, "SHA256SUMS.txt has an unexpected number of entries");
  const checksumNames = [];
  for (const line of checksumLines) {
    const match = line.match(/^([0-9a-f]{64})  ([^\\/]+)$/);
    assert.ok(match, `Invalid SHA256SUMS.txt line: ${line}`);
    const [, expectedHash, name] = match;
    assert.ok(expectedChecksumNames.includes(name), `Unexpected checksum entry: ${name}`);
    assert.equal(createHash("sha256").update(contents[name]).digest("hex"), expectedHash, `Checksum mismatch: ${name}`);
    checksumNames.push(name);
  }
  assert.deepEqual(checksumNames.sort(), [...expectedChecksumNames].sort(), "SHA256SUMS.txt is missing an expected asset");

  const desktopPath = join(root, "target", "package-build", "release", "todolist-desktop.exe");
  const mcpPath = join(root, "target", "package-build", "release", "todolist-mcp.exe");
  if (verifyExecutables) {
    const desktop = await readFile(desktopPath);
    const mcp = await readFile(mcpPath);
    assert.equal(readPeSubsystem(desktop, "Desktop executable"), 2, "Desktop executable must use the Windows GUI subsystem");
    assert.equal(readPeSubsystem(mcp, "MCP executable"), 3, "MCP executable must use the console subsystem");
    for (const [content, label] of [[contents[installerName], installerName], [desktop, basename(desktopPath)], [mcp, basename(mcpPath)]]) {
      assertNoPrivateBuildPath(content, label);
    }
    runIdentityChecks(desktopPath, mcpPath, version);
  }

  const artifacts = await Promise.all(publicAssetNames.map(async (name) => {
    const metadata = await stat(paths[name]);
    return { name, bytes: metadata.size, sha256: createHash("sha256").update(contents[name]).digest("hex") };
  }));
  return { version, tag, repository, signature: "verified", manifest: "verified", checksums: "verified", artifacts };
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const args = parseArguments(process.argv.slice(2));
  const root = resolve(args.root ?? fileURLToPath(new URL("..", import.meta.url)));
  const repository = args.repository ?? process.env.GITHUB_REPOSITORY;
  assert.ok(args.tag, "--tag is required");
  assert.ok(repository, "--repository or GITHUB_REPOSITORY is required");
  console.log(JSON.stringify(await verifyRelease({ root, tag: args.tag, repository }), null, 2));
}
