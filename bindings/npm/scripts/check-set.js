#!/usr/bin/env node
"use strict";

// Checks the whole set of npm packages together, as the release workflow's
// `npm-package` job does for pip with `twine check`:
//
//   node scripts/check-set.js --dir <folder of the nine packed .tgz files> \
//     --version <the one version> [--install]
//
// It reads the packed files themselves, not the folders they were made from,
// so it checks what would be published. With --install it also installs the
// launcher and this machine's platform package, from the files only, into a
// scratch project, and runs both command names. Nothing is published and the
// registry is not asked for anything the install needs.

const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { PLATFORMS, platformKey, binaryName } = require("../anything2mcp/lib/platform.js");

const LAUNCHER = "anything2mcp";
const NPM = process.platform === "win32" ? "npm.cmd" : "npm";

const problems = [];
function fail(message) {
  problems.push(message);
}

function args(argv) {
  const out = { install: false };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--install") out.install = true;
    else if (argv[i] === "--dir" || argv[i] === "--version") {
      out[argv[i].slice(2)] = argv[i + 1];
      i += 1;
    } else throw new Error(`unknown argument ${argv[i]}`);
  }
  if (!out.dir || !out.version) throw new Error("usage: check-set.js --dir <folder> --version <version> [--install]");
  return out;
}

function walk(dir, base = dir) {
  const files = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(full, base));
    else files.push({ rel: path.relative(base, full).split(path.sep).join("/"), full, entry });
  }
  return files;
}

// Whether a file is one the package's `files` list lets in. npm always adds
// package.json; every other file must be named, or sit under a named folder.
function allowed(rel, list) {
  if (rel === "package.json") return true;
  return list.some((item) => (item.endsWith("/") ? rel.startsWith(item) : rel === item));
}

function unpack(tgz, into) {
  fs.mkdirSync(into, { recursive: true });
  execFileSync("tar", ["-xzf", tgz, "-C", into]);
  return path.join(into, "package");
}

// The Rust target names its processor, system, and libc; the platform key
// must say the same.
function targetMatchesKey(rust, p) {
  const cpu = rust.startsWith("x86_64") ? "x64" : rust.startsWith("aarch64") ? "arm64" : null;
  const system = rust.includes("-linux-") ? "linux" : rust.includes("-darwin") ? "darwin" : rust.includes("-windows-") ? "win32" : null;
  const libc = rust.endsWith("-musl") ? "musl" : rust.endsWith("-gnu") ? "glibc" : undefined;
  return cpu === p.cpu && system === p.os && libc === p.libc;
}

function main() {
  const given = args(process.argv.slice(2));
  const { version, install } = given;
  // An absolute folder: the install below runs in a folder of its own, and
  // npm reads a relative `folder/file.tgz` as the short name of a
  // repository on a code host, not as a file.
  const dir = path.resolve(given.dir);
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "a2m-npm-check-"));
  const tgzs = fs.readdirSync(dir).filter((name) => name.endsWith(".tgz")).sort();
  const expectedNames = [LAUNCHER, ...Object.values(PLATFORMS).map((p) => p.name)].sort();
  if (tgzs.length !== 9) fail(`expected 9 packed files (the launcher and 8 platform packages); found ${tgzs.length}: ${tgzs.join(", ")}`);

  const byName = {};
  tgzs.forEach((name, index) => {
    const root = unpack(path.join(dir, name), path.join(scratch, String(index)));
    const manifest = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
    if (byName[manifest.name]) fail(`${manifest.name} is packed twice`);
    byName[manifest.name] = { tgz: path.join(dir, name), root, manifest };
  });
  const found = Object.keys(byName).sort();
  if (JSON.stringify(found) !== JSON.stringify(expectedNames)) {
    fail(`the packages present are not the launcher and the 8 platform packages.\n  present:  ${found.join(", ")}\n  expected: ${expectedNames.join(", ")}`);
  }

  for (const [name, pkg] of Object.entries(byName)) {
    const { manifest, root } = pkg;
    if (manifest.version !== version) fail(`${name} states version ${manifest.version}, not ${version}`);
    const list = manifest.files;
    if (!Array.isArray(list) || list.length === 0) {
      fail(`${name} has no files list, so npm would pack whatever is in the folder`);
      continue;
    }
    const files = walk(root);
    for (const file of files) {
      if (!allowed(file.rel, list)) fail(`${name} holds ${file.rel}, which is outside its files list (${list.join(", ")})`);
      if (file.entry.isSymbolicLink()) fail(`${name} holds a link, ${file.rel}`);
    }
    for (const item of list) {
      if (!files.some((file) => (item.endsWith("/") ? file.rel.startsWith(item) : file.rel === item))) {
        fail(`${name} names ${item} in its files list and holds nothing there`);
      }
    }
  }

  const launcher = byName[LAUNCHER];
  if (launcher) {
    const optional = launcher.manifest.optionalDependencies || {};
    const want = Object.values(PLATFORMS).map((p) => p.name).sort();
    if (JSON.stringify(Object.keys(optional).sort()) !== JSON.stringify(want)) {
      fail(`the launcher's optional dependencies are ${Object.keys(optional).sort().join(", ")}; they must be exactly the 8 platform packages`);
    }
    for (const [name, range] of Object.entries(optional)) {
      if (range !== version) fail(`the launcher asks for ${name} at ${range}, not exactly ${version}`);
    }
    if (launcher.manifest.dependencies && Object.keys(launcher.manifest.dependencies).length > 0) {
      fail("the launcher has dependencies of its own; it must have none");
    }
    for (const bin of ["anything2mcp", "a2m"]) {
      if (!launcher.manifest.bin || !launcher.manifest.bin[bin]) fail(`the launcher names no command ${bin}`);
    }
  }

  for (const [key, p] of Object.entries(PLATFORMS)) {
    const pkg = byName[p.name];
    if (!pkg) continue;
    const { manifest, root } = pkg;
    if (!targetMatchesKey(p.rust, p)) fail(`${key}: the table's Rust target ${p.rust} does not match its os, cpu, and libc`);
    if (JSON.stringify(manifest.os) !== JSON.stringify([p.os])) fail(`${p.name} has os ${JSON.stringify(manifest.os)}, expected ["${p.os}"]`);
    if (JSON.stringify(manifest.cpu) !== JSON.stringify([p.cpu])) fail(`${p.name} has cpu ${JSON.stringify(manifest.cpu)}, expected ["${p.cpu}"]`);
    const libc = p.libc ? [p.libc] : undefined;
    if (JSON.stringify(manifest.libc) !== JSON.stringify(libc)) fail(`${p.name} has libc ${JSON.stringify(manifest.libc)}, expected ${JSON.stringify(libc)}`);
    const binaries = walk(path.join(root, "bin")).map((file) => file);
    if (binaries.length !== 1 || binaries[0].rel !== binaryName(p.os)) {
      fail(`${p.name} must hold exactly bin/${binaryName(p.os)}; it holds ${binaries.map((b) => b.rel).join(", ") || "nothing"}`);
    } else {
      const stat = fs.statSync(binaries[0].full);
      if (stat.size === 0) fail(`${p.name}: the binary is empty`);
      if (p.os !== "win32" && (stat.mode & 0o111) !== 0o111) fail(`${p.name}: the binary lacks the executable bit (mode ${(stat.mode & 0o777).toString(8)})`);
    }
  }

  if (install && problems.length === 0) tryInstall(byName, version, scratch);

  fs.rmSync(scratch, { recursive: true, force: true });
  if (problems.length > 0) {
    for (const problem of problems) process.stderr.write(`::error::${problem}\n`);
    process.exit(1);
  }
  process.stdout.write(`The set is whole: 1 launcher and 8 platform packages at ${version}${install ? ", and the launcher installs and runs" : ""}.\n`);
}

// Installs the launcher and this machine's platform package from the packed
// files alone. The registry address points at a closed port, so an install
// that needed the registry for either of them fails. The other seven
// optional packages cannot be reached and are skipped, as on any machine
// they do not fit.
function tryInstall(byName, version, scratch) {
  const key = platformKey();
  const own = PLATFORMS[key];
  if (!own || !byName[own.name]) return fail(`this machine is ${key}; there is no platform package for it in the set`);
  const project = path.join(scratch, "project");
  fs.mkdirSync(project);
  fs.writeFileSync(path.join(project, "package.json"), JSON.stringify({ name: "a2m-install-check", version: "0.0.0", private: true }));
  try {
    execFileSync(
      NPM,
      [
        "install", byName[LAUNCHER].tgz, byName[own.name].tgz,
        "--no-audit", "--no-fund", "--no-package-lock", "--fetch-retries=0",
        "--registry", "http://127.0.0.1:9/",
      ],
      { cwd: project, encoding: "utf8", stdio: "pipe", shell: process.platform === "win32" },
    );
  } catch (error) {
    return fail(`installing from the packed files failed:\n${error.stdout || ""}${error.stderr || ""}`);
  }
  const modules = path.join(project, "node_modules");
  if (!fs.existsSync(path.join(modules, ...own.name.split("/"), "bin", binaryName(own.os)))) fail(`${own.name} did not arrive`);
  for (const [otherKey, other] of Object.entries(PLATFORMS)) {
    if (otherKey !== key && fs.existsSync(path.join(modules, ...other.name.split("/")))) fail(`${other.name} was installed on a ${key} machine`);
  }
  for (const command of ["anything2mcp", "a2m"]) {
    const exe = path.join(modules, ".bin", process.platform === "win32" ? `${command}.cmd` : command);
    try {
      const out = execFileSync(exe, ["--version"], { encoding: "utf8", shell: process.platform === "win32" });
      if (!out.includes(version)) fail(`${command} --version printed "${out.trim()}", which does not hold ${version}`);
      process.stdout.write(`${command} --version: ${out.trim()}\n`);
    } catch (error) {
      fail(`${command} --version failed: ${error.message}`);
    }
  }
}

try {
  main();
} catch (error) {
  process.stderr.write(`error: ${error.message}\n`);
  process.exit(2);
}
