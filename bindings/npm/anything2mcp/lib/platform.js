"use strict";

// Which platform package holds the binary for this machine, and where the
// binary is inside it. One table, read by the launcher, the assembly script,
// and the tests, so the names are written once.

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

// Whether this Linux uses musl: Node reports the glibc version it runs on,
// and reports none on musl.
function isMusl() {
  if (process.platform !== "linux") return false;
  try {
    const report = process.report && process.report.getReport();
    const header = report && (typeof report === "string" ? JSON.parse(report) : report).header;
    return !(header && header.glibcVersionRuntime);
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

module.exports = { PLATFORMS, platformKey, binaryName, binaryInPackage, isMusl };
