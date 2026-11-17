"use strict";

// Which platform package holds the binary for this machine, and where the
// binary is inside it. One table, read by the launcher, the assembly script,
// and the tests, so the names are written once.

const fs = require("node:fs");
const path = require("node:path");

// `<process.platform>-<process.arch>[-musl]` to the platform package's name.
// Each package's `os`, `cpu`, and (on Linux) `libc` fields match its key, so
// the package manager installs only the one that fits.
const PLATFORMS = {
  "linux-x64": { name: "@anything2mcp/linux-x64", os: "linux", cpu: "x64", libc: "glibc", rust: "x86_64-unknown-linux-gnu" },
  "linux-arm64": { name: "@anything2mcp/linux-arm64", os: "linux", cpu: "arm64", libc: "glibc", rust: "aarch64-unknown-linux-gnu" },
  "linux-x64-musl": { name: "@anything2mcp/linux-x64-musl", os: "linux", cpu: "x64", libc: "musl", rust: "x86_64-unknown-linux-musl" },
  "linux-arm64-musl": { name: "@anything2mcp/linux-arm64-musl", os: "linux", cpu: "arm64", libc: "musl", rust: "aarch64-unknown-linux-musl" },
  "darwin-x64": { name: "@anything2mcp/darwin-x64", os: "darwin", cpu: "x64", rust: "x86_64-apple-darwin" },
  "darwin-arm64": { name: "@anything2mcp/darwin-arm64", os: "darwin", cpu: "arm64", rust: "aarch64-apple-darwin" },
  "win32-x64": { name: "@anything2mcp/win32-x64", os: "win32", cpu: "x64", rust: "x86_64-pc-windows-msvc" },
  "win32-arm64": { name: "@anything2mcp/win32-arm64", os: "win32", cpu: "arm64", rust: "aarch64-pc-windows-msvc" },
};

// Whether this Linux uses musl: a musl system has its loader in /lib,
// named ld-musl-<cpu>.so.1. One directory read; asking Node for its report,
// which also says, costs several milliseconds at every start.
function isMusl() {
  if (process.platform !== "linux") return false;
  try {
    return fs.readdirSync("/lib").some((name) => name.startsWith("ld-musl-"));
  } catch {
    return false;
  }
}

function platformKey(platform = process.platform, arch = process.arch, musl = isMusl()) {
  return `${platform}-${arch}${platform === "linux" && musl ? "-musl" : ""}`;
}

function binaryName(platform = process.platform) {
  return platform === "win32" ? "anything2mcp.exe" : "anything2mcp";
}

// The binary's path inside a platform package.
function binaryInPackage(packageDir, platform = process.platform) {
  return path.join(packageDir, "bin", binaryName(platform));
}

// The folders a package manager puts the launcher's own dependency in, and
// no other: inside the launcher's package, as npm does when it nests one,
// and beside it in the `node_modules` folder that holds it, as npm, Yarn,
// and pnpm do when they hoist one. Node's own resolution would go on to the
// `node_modules` of every folder above, `NODE_PATH`, and the global
// folders, where a package of the same name may be anybody's.
function candidateDirs(launcherRoot, name) {
  const dirs = [path.join(launcherRoot, "node_modules", ...name.split("/"))];
  const holder = path.dirname(launcherRoot);
  if (path.basename(holder) === "node_modules") {
    dirs.push(path.join(holder, ...name.split("/")));
  }
  return dirs;
}

// Finds the platform package that belongs to this launcher. Returns
// `{ dir, binary }`, or `{ missing: true }` when none is installed where the
// launcher's own dependencies go, or `{ refused: reason }` for a package
// that is there and is not this launcher's: another name, another version,
// or a program that lies outside the package. `resolve` is Node's
// resolution, used only under Yarn's Plug'n'Play, which resolves a package
// from the launcher's declared dependencies alone.
function findPlatformPackage(launcherRoot, platform, version, { pnp = false, resolve, system = process.platform } = {}) {
  let dirs = candidateDirs(launcherRoot, platform.name);
  if (pnp && typeof resolve === "function") {
    try {
      dirs = [path.dirname(resolve(`${platform.name}/package.json`))];
    } catch {
      dirs = [];
    }
  }
  const dir = dirs.find((candidate) => fs.existsSync(path.join(candidate, "package.json")));
  if (dir === undefined) return { missing: true };
  let manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
  } catch {
    return { refused: `the package at ${dir} has no readable package.json` };
  }
  if (manifest.name !== platform.name || manifest.version !== version) {
    return {
      refused: `the package at ${dir} is ${manifest.name}@${manifest.version}, and this launcher needs ${platform.name}@${version}`,
    };
  }
  const binary = binaryInPackage(dir, system);
  let real;
  let home;
  try {
    real = fs.realpathSync(binary);
    home = fs.realpathSync(dir);
  } catch {
    return { refused: `the package ${platform.name} holds no program at ${binary}` };
  }
  if (!real.startsWith(home + path.sep) || !fs.statSync(real).isFile()) {
    return { refused: `the program at ${binary} lies outside the package ${platform.name}` };
  }
  return { dir, binary: real };
}

module.exports = { PLATFORMS, platformKey, binaryName, binaryInPackage, candidateDirs, findPlatformPackage, isMusl };
