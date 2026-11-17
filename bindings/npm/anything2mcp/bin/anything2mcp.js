#!/usr/bin/env node
"use strict";

// The launcher: finds the binary in the platform package that the package
// manager installed for this machine, and runs it with the same arguments,
// input, and output. It downloads nothing and needs no install script. When
// the platform package is missing it says which one to install, and exits 4.
// `a2m` is the same file.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const { PLATFORMS, platformKey, binaryInPackage } = require("../lib/platform.js");

function fail(message, fix) {
  process.stderr.write(`error: ${message}\nfix: ${fix}\n`);
  process.exit(4);
}

const key = platformKey();
const platform = PLATFORMS[key];
if (!platform) {
  fail(
    `anything2mcp has no build for this machine (${key}).`,
    "Build it from the source at https://github.com/MHughesDev/anything2mcp.",
  );
}

let packageDir;
try {
  packageDir = path.dirname(require.resolve(`${platform.name}/package.json`));
} catch {
  const version = require("../package.json").version;
  fail(
    `the package ${platform.name}, which holds the anything2mcp program for this machine, is not installed. ` +
      "It is an optional dependency, so installing with optional dependencies left out skips it.",
    `Install it with \`npm install ${platform.name}@${version}\`, or install anything2mcp again without \`--omit=optional\` or \`--no-optional\`.`,
  );
}

const binary = binaryInPackage(packageDir);
if (!fs.existsSync(binary)) {
  fail(`the package ${platform.name} holds no program at ${binary}.`, `Install ${platform.name} again.`);
}

const child = spawnSync(binary, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (child.error) {
  fail(`the anything2mcp program could not be started: ${child.error.message}.`, `Install ${platform.name} again.`);
}
if (child.signal) {
  // Ends the same way the program did.
  process.kill(process.pid, child.signal);
}
process.exit(child.status === null ? 1 : child.status);
