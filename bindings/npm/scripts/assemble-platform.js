#!/usr/bin/env node
"use strict";

// Assembles one platform package from a built binary, ready for `npm pack`.
//
//   node scripts/assemble-platform.js --platform linux-x64 \
//     --binary ../../target/release/anything2mcp --out ../../dist/npm/linux-x64
//
// The version is the launcher's, so every package carries the same one.
// Nothing is compiled here: the binary is the one the release built.

const fs = require("node:fs");
const path = require("node:path");
const { PLATFORMS, binaryName } = require("../anything2mcp/lib/platform.js");

const HERE = path.resolve(__dirname, "..");

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    if (!flag.startsWith("--") || i + 1 >= argv.length) {
      throw new Error(`expected --name value pairs, found ${flag}`);
    }
    out[flag.slice(2)] = argv[i + 1];
  }
  return out;
}

function fill(template, values) {
  return template.replace(/\{\{(\w+)\}\}/g, (_, name) => {
    if (!(name in values)) throw new Error(`the template names {{${name}}}, which has no value`);
    return values[name];
  });
}

function assemble({ platform: key, binary, out }) {
  const platform = PLATFORMS[key];
  if (!platform) {
    throw new Error(`no platform ${key}; known: ${Object.keys(PLATFORMS).join(", ")}`);
  }
  if (!binary || !fs.statSync(binary).isFile()) {
    throw new Error(`--binary must name the built anything2mcp program`);
  }
  if (!out) throw new Error("--out must name the folder to write the package to");
  const version = JSON.parse(fs.readFileSync(path.join(HERE, "anything2mcp", "package.json"), "utf8")).version;
  const values = {
    name: platform.name,
    version,
    key,
    os: platform.os,
    cpu: platform.cpu,
    libc: platform.libc ? `,\n  "libc": ["${platform.libc}"]` : "",
  };
  fs.rmSync(out, { recursive: true, force: true });
  fs.mkdirSync(path.join(out, "bin"), { recursive: true });
  const manifest = fill(fs.readFileSync(path.join(HERE, "platform", "package.json.template"), "utf8"), values);
  JSON.parse(manifest); // refuses a template that does not fill to JSON
  fs.writeFileSync(path.join(out, "package.json"), manifest);
  fs.writeFileSync(
    path.join(out, "README.md"),
    fill(fs.readFileSync(path.join(HERE, "platform", "README.md.template"), "utf8"), values),
  );
  fs.copyFileSync(path.join(HERE, "platform", "LICENSE"), path.join(out, "LICENSE"));
  const target = path.join(out, "bin", binaryName(platform.os));
  fs.copyFileSync(binary, target);
  fs.chmodSync(target, 0o755);
  return { name: platform.name, version, dir: out };
}

if (require.main === module) {
  try {
    const made = assemble(args(process.argv.slice(2)));
    process.stdout.write(`${made.name}@${made.version} in ${made.dir}\n`);
  } catch (error) {
    process.stderr.write(`error: ${error.message}\n`);
    process.exit(2);
  }
}

module.exports = { assemble };
