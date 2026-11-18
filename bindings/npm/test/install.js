#!/usr/bin/env node
"use strict";

// Proves the npm packages install and run as the design says, with no
// dependency of its own and nothing fetched from the internet:
//
//   node test/install.js --binary <a built anything2mcp for this machine>
//
// It assembles every platform package (this machine's with the real binary;
// the others with a stand-in file, since the package manager only reads
// their manifests and must not install them), packs them and the launcher,
// serves them from a small registry on 127.0.0.1, and installs `anything2mcp`
// into a scratch project three ways:
//   (a) normally: both names run the program;
//   (b) with install scripts disabled (--ignore-scripts): the same;
//   (c) with optional dependencies left out (--omit=optional): the command
//       names the package to install and exits 4.
// In (a) it also checks that only this machine's platform package arrived.

const { execFileSync, spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const { PLATFORMS, platformKey } = require("../anything2mcp/lib/platform.js");
const { assemble } = require("../scripts/assemble-platform.js");

const HERE = path.resolve(__dirname, "..");
const NPM = process.platform === "win32" ? "npm.cmd" : "npm";

function binaryArg() {
  const at = process.argv.indexOf("--binary");
  const binary = at > 0 ? process.argv[at + 1] : process.env.A2M_TEST_BINARY;
  if (!binary) {
    process.stderr.write("usage: node test/install.js --binary <path to a built anything2mcp>\n");
    process.exit(2);
  }
  return path.resolve(binary);
}

function npm(args, cwd) {
  // `shell` only on Windows, where npm is a batch file.
  return execFileSync(NPM, args, { cwd, encoding: "utf8", shell: process.platform === "win32" });
}

// The registry answers in this process, so anything that talks to it must
// not block the event loop: these run asynchronously.
function runAsync(command, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, shell: process.platform === "win32", windowsHide: true });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

function pack(dir, into) {
  const out = npm(["pack", "--json", "--pack-destination", into], dir);
  return path.join(into, JSON.parse(out)[0].filename);
}

function registry(tarballs) {
  // name -> { manifest, file }
  const server = http.createServer((req, res) => {
    const url = decodeURIComponent(req.url.split("?")[0]).replace(/^\/+/, "");
    const tarball = Object.values(tarballs).find((entry) => url === `tarballs/${path.basename(entry.file)}`);
    if (tarball) {
      res.writeHead(200, { "content-type": "application/octet-stream" });
      res.end(fs.readFileSync(tarball.file));
      return;
    }
    const entry = tarballs[url];
    if (!entry) {
      res.writeHead(404, { "content-type": "application/json" });
      res.end("{}");
      return;
    }
    const bytes = fs.readFileSync(entry.file);
    const { address, port } = server.address();
    const version = entry.manifest.version;
    const body = {
      name: entry.manifest.name,
      "dist-tags": { latest: version },
      versions: {
        [version]: {
          ...entry.manifest,
          dist: {
            tarball: `http://${address}:${port}/tarballs/${path.basename(entry.file)}`,
            shasum: crypto.createHash("sha1").update(bytes).digest("hex"),
            integrity: `sha512-${crypto.createHash("sha512").update(bytes).digest("base64")}`,
          },
        },
      },
    };
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

function check(condition, message) {
  if (!condition) throw new Error(message);
}

async function main() {
  const binary = binaryArg();
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "a2m-npm-"));
  const server = { close() {} };
  let running = server;
  try {
    const here = platformKey();
    check(PLATFORMS[here], `no platform package for this machine (${here})`);
    const stand = path.join(work, "stand-in");
    fs.writeFileSync(stand, "not a program: this platform package is only read, never installed\n");
    const packed = path.join(work, "packed");
    fs.mkdirSync(packed);
    const tarballs = {};
    for (const key of Object.keys(PLATFORMS)) {
      const made = assemble({ platform: key, binary: key === here ? binary : stand, out: path.join(work, "pkg", key) });
      const file = pack(made.dir, packed);
      tarballs[made.name] = { manifest: JSON.parse(fs.readFileSync(path.join(made.dir, "package.json"))), file };
    }
    const launcherDir = path.join(HERE, "anything2mcp");
    const launcher = JSON.parse(fs.readFileSync(path.join(launcherDir, "package.json")));
    tarballs[launcher.name] = { manifest: launcher, file: pack(launcherDir, packed) };
    running = await registry(tarballs);
    const { address, port } = running.address();
    const url = `http://${address}:${port}/`;

    const modes = [
      { label: "(a) a normal install", flags: [], runs: true },
      { label: "(b) install scripts disabled", flags: ["--ignore-scripts"], runs: true },
      { label: "(c) optional dependencies left out", flags: ["--omit=optional"], runs: false },
    ];
    for (const mode of modes) {
      const project = path.join(work, `project-${modes.indexOf(mode)}`);
      fs.mkdirSync(project);
      fs.writeFileSync(path.join(project, "package.json"), JSON.stringify({ name: "scratch", private: true }));
      // The registry for every name, a cache of its own, and nothing else.
      fs.writeFileSync(
        path.join(project, ".npmrc"),
        `registry=${url}\n@anything2mcp:registry=${url}\ncache=${path.join(work, "cache")}\naudit=false\nfund=false\nupdate-notifier=false\n`,
      );
      const installed = await runAsync(NPM, ["install", `anything2mcp@${launcher.version}`, ...mode.flags], project);
      check(installed.status === 0, `${mode.label}: npm install failed: ${installed.stderr}`);
      const scoped = path.join(project, "node_modules", "@anything2mcp");
      const present = fs.existsSync(scoped) ? fs.readdirSync(scoped) : [];
      const bin = path.join(project, "node_modules", ".bin");
      for (const name of ["anything2mcp", "a2m"]) {
        const command = path.join(bin, process.platform === "win32" ? `${name}.cmd` : name);
        const run = await runAsync(command, ["--version"], project);
        if (mode.runs) {
          check(run.status === 0, `${mode.label}: \`${name} --version\` exited ${run.status}: ${run.stderr}`);
          check(run.stdout.trim() === `anything2mcp ${launcher.version}`, `${mode.label}: \`${name}\` printed ${run.stdout}`);
        } else {
          check(run.status === 4, `${mode.label}: \`${name}\` exited ${run.status}, not 4`);
          check(
            run.stderr.includes(`npm install ${PLATFORMS[here].name}@${launcher.version}`),
            `${mode.label}: \`${name}\` did not name the package to install: ${run.stderr}`,
          );
        }
      }
      const want = mode.runs ? [PLATFORMS[here].name.split("/")[1]] : [];
      check(
        JSON.stringify(present) === JSON.stringify(want),
        `${mode.label}: installed platform packages ${JSON.stringify(present)}, expected ${JSON.stringify(want)}`,
      );
      process.stdout.write(`ok ${mode.label}\n`);
    }
  } finally {
    running.close();
    fs.rmSync(work, { recursive: true, force: true });
  }
}

main().catch((error) => {
  process.stderr.write(`not ok: ${error.message}\n`);
  process.exit(1);
});
