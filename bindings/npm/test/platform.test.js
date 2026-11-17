"use strict";

// Run: node --test test/platform.test.js
// The table of platforms and the launcher's manifest must agree, so that a
// platform package named in one is never missing from the other.

const test = require("node:test");
const assert = require("node:assert");
const path = require("node:path");
const { PLATFORMS, platformKey, binaryName } = require("../anything2mcp/lib/platform.js");

const launcher = require(path.join(__dirname, "..", "anything2mcp", "package.json"));

test("the launcher lists every platform package, at its own version, as optional", () => {
  const listed = Object.entries(launcher.optionalDependencies).sort();
  const wanted = Object.values(PLATFORMS)
    .map((platform) => [platform.name, launcher.version])
    .sort();
  assert.deepStrictEqual(listed, wanted);
  assert.strictEqual(launcher.dependencies, undefined, "the launcher has no dependency of its own");
  assert.strictEqual(launcher.scripts, undefined, "no install script, and no other");
  assert.deepStrictEqual(launcher.bin, { anything2mcp: "bin/anything2mcp.js", a2m: "bin/anything2mcp.js" });
});

test("each machine names one platform package, and the binary's name follows the system", () => {
  assert.strictEqual(platformKey("linux", "x64", false), "linux-x64");
  assert.strictEqual(platformKey("linux", "arm64", true), "linux-arm64-musl");
  assert.strictEqual(platformKey("darwin", "arm64", true), "darwin-arm64");
  assert.strictEqual(platformKey("win32", "x64", false), "win32-x64");
  for (const [key, platform] of Object.entries(PLATFORMS)) {
    assert.ok(key.startsWith(`${platform.os}-${platform.cpu}`), key);
    assert.strictEqual(platform.os === "linux", Boolean(platform.libc), key);
  }
  assert.strictEqual(binaryName("win32"), "anything2mcp.exe");
  assert.strictEqual(binaryName("linux"), "anything2mcp");
});

// ---- Which platform package the launcher runs ------------------------------

const fs = require("node:fs");
const os = require("node:os");
const { spawn } = require("node:child_process");
const { findPlatformPackage } = require("../anything2mcp/lib/platform.js");

const WINDOWS = process.platform === "win32";
const HERE_PLATFORM = PLATFORMS[platformKey()] || PLATFORMS["linux-x64"];

// A platform package at `dir`, of `version`, whose program is `program`.
function platformPackage(dir, version, program = "#!/bin/sh\nexit 0\n") {
  fs.mkdirSync(path.join(dir, "bin"), { recursive: true });
  fs.writeFileSync(path.join(dir, "package.json"), JSON.stringify({ name: HERE_PLATFORM.name, version }));
  const binary = path.join(dir, "bin", binaryName());
  fs.writeFileSync(binary, program, { mode: 0o755 });
  return binary;
}

// A project with the launcher installed in its `node_modules`.
function project() {
  const top = fs.mkdtempSync(path.join(os.tmpdir(), "a2m-launch-"));
  const modules = path.join(top, "project", "node_modules");
  const root = path.join(modules, "anything2mcp");
  fs.mkdirSync(path.join(root, "bin"), { recursive: true });
  fs.mkdirSync(path.join(root, "lib"), { recursive: true });
  const source = path.join(__dirname, "..", "anything2mcp");
  fs.copyFileSync(path.join(source, "bin", "anything2mcp.js"), path.join(root, "bin", "anything2mcp.js"));
  fs.copyFileSync(path.join(source, "lib", "platform.js"), path.join(root, "lib", "platform.js"));
  fs.copyFileSync(path.join(source, "package.json"), path.join(root, "package.json"));
  return { top, modules, root };
}

function hoisted(modules) {
  return path.join(modules, ...HERE_PLATFORM.name.split("/"));
}

test("the launcher's platform package is found only where its own dependencies are installed", (t) => {
  const { top, modules, root } = project();
  t.after(() => fs.rmSync(top, { recursive: true, force: true }));
  const version = launcher.version;

  // One planted in the node_modules of a folder above the project is not
  // the launcher's, and is not found.
  platformPackage(hoisted(path.join(top, "node_modules")), version);
  assert.deepStrictEqual(findPlatformPackage(root, HERE_PLATFORM, version), { missing: true });

  // Hoisted beside the launcher, as npm, Yarn, and pnpm put it: found.
  const beside = platformPackage(hoisted(modules), version);
  assert.strictEqual(findPlatformPackage(root, HERE_PLATFORM, version).binary, fs.realpathSync(beside));

  // Nested inside the launcher's own folder: found first.
  const nested = platformPackage(hoisted(path.join(root, "node_modules")), version);
  assert.strictEqual(findPlatformPackage(root, HERE_PLATFORM, version).binary, fs.realpathSync(nested));
});

test("a platform package of another version, or whose program lies outside it, is refused", (t) => {
  const { top, modules, root } = project();
  t.after(() => fs.rmSync(top, { recursive: true, force: true }));
  platformPackage(hoisted(modules), "9.9.9");
  const other = findPlatformPackage(root, HERE_PLATFORM, launcher.version);
  assert.match(other.refused, /9\.9\.9/);

  if (!WINDOWS) {
    fs.rmSync(hoisted(modules), { recursive: true });
    const binary = platformPackage(hoisted(modules), launcher.version);
    const elsewhere = path.join(top, "elsewhere");
    fs.writeFileSync(elsewhere, "#!/bin/sh\nexit 0\n", { mode: 0o755 });
    fs.rmSync(binary);
    fs.symlinkSync(elsewhere, binary);
    assert.match(findPlatformPackage(root, HERE_PLATFORM, launcher.version).refused, /outside/);
  }
});

test("the launcher refuses a planted package and passes a stop signal on to the program", { skip: WINDOWS }, async (t) => {
  const { top, modules, root } = project();
  t.after(() => fs.rmSync(top, { recursive: true, force: true }));
  const launch = path.join(root, "bin", "anything2mcp.js");

  // Only a package in a folder above: refused as missing, exit 4.
  platformPackage(hoisted(path.join(top, "node_modules")), launcher.version, "#!/bin/sh\necho planted\n");
  const planted = await new Promise((resolve) => {
    const child = spawn(process.execPath, [launch], { stdio: ["ignore", "pipe", "pipe"] });
    let out = "";
    child.stdout.on("data", (chunk) => (out += chunk));
    child.on("exit", (status) => resolve({ status, out }));
  });
  assert.strictEqual(planted.status, 4);
  assert.ok(!planted.out.includes("planted"), planted.out);

  // The launcher's own: a program that says when it is asked to stop.
  const told = path.join(top, "told");
  const program = `#!${process.execPath}
process.on("SIGTERM", () => { require("fs").writeFileSync(${JSON.stringify(told)}, "term"); process.exit(0); });
process.stdout.write("ready\\n");
setInterval(() => {}, 1000);
`;
  platformPackage(hoisted(modules), launcher.version, program);
  const child = spawn(process.execPath, [launch, "serve"], { stdio: ["ignore", "pipe", "inherit"] });
  await new Promise((resolve) => child.stdout.once("data", resolve));
  const ended = new Promise((resolve) => child.on("exit", (status, signal) => resolve({ status, signal })));
  child.kill("SIGTERM");
  assert.deepStrictEqual(await ended, { status: 0, signal: null });
  assert.strictEqual(fs.readFileSync(told, "utf8"), "term");
});
