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
