#!/usr/bin/env node
"use strict";

// The launcher: finds the binary in the platform package that the package
// manager installed for this machine, and runs it with the same arguments,
// input, and output. It downloads nothing and needs no install script. When
// the platform package is missing it says which one to install, and exits 4.
// `a2m` is the same file.
//
// The platform package is looked for only where the launcher's own
// dependencies are installed, never in the folders above it, and only a
// package of the launcher's own version is run. The signals a process
// manager or an MCP client sends to stop the program are passed on to it,
// and the launcher ends as the program ends.

const { spawn } = require("node:child_process");
const path = require("node:path");
const { PLATFORMS, platformKey, findPlatformPackage } = require("../lib/platform.js");

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

const root = path.resolve(__dirname, "..");
const version = require("../package.json").version;
const found = findPlatformPackage(root, platform, version, {
  pnp: Boolean(process.versions.pnp),
  resolve: (request) => require.resolve(request),
});
if (found.missing) {
  fail(
    `the package ${platform.name}, which holds the anything2mcp program for this machine, is not installed. ` +
      "It is an optional dependency, so installing with optional dependencies left out skips it.",
    `Install it with \`npm install ${platform.name}@${version}\`, or install anything2mcp again without \`--omit=optional\` or \`--no-optional\`.`,
  );
}
if (found.refused) {
  fail(`${found.refused}, so it is not run.`, `Install anything2mcp again, so that ${platform.name}@${version} is installed with it.`);
}

const child = spawn(found.binary, process.argv.slice(2), { stdio: "inherit", windowsHide: true });

// On Windows the console sends Ctrl-C to the program itself, and Node's
// `kill` there ends a process at once: the launcher only waits. Elsewhere a
// signal sent to the launcher alone is passed on.
const SIGNALS = process.platform === "win32" ? ["SIGINT", "SIGBREAK"] : ["SIGINT", "SIGTERM", "SIGHUP"];
const forward = (signal) => {
  if (process.platform !== "win32" && child.exitCode === null && child.signalCode === null) {
    child.kill(signal);
  }
};
for (const signal of SIGNALS) process.on(signal, forward);

child.on("error", (error) => {
  fail(`the anything2mcp program could not be started: ${error.message}.`, `Install ${platform.name} again.`);
});
child.on("exit", (status, signal) => {
  for (const name of SIGNALS) process.removeListener(name, forward);
  if (signal) {
    // Ends the same way the program did.
    process.kill(process.pid, signal);
    return;
  }
  process.exit(status === null ? 1 : status);
});
