"use strict";

// Anything2MCP for Node: a thin client of the local engine.
//
//   const { connect } = require("anything2mcp");
//   const client = await connect({ project: "." });
//   const status = await client.engineStatus();
//   console.log(status.ok, status.result);
//   client.close();
//
// The client holds no engine logic. It sends an action and its input to the
// engine that runs as this user, over the local channel, and returns the
// engine's response as it was written. When no engine runs, `connect` runs
// the installed program's `anything2mcp engine start`, as any command would
// start one, unless it is given `start: false`. The methods of `Actions` are
// generated from the engine's OpenAPI document.

const { execFile } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const { PLATFORMS, platformKey, findPlatformPackage } = require("../lib/platform.js");

const { ACTIONS, Actions } = require("./actions.js");
const { Client, LOCAL_API, connectTo, greet } = require("./client.js");
const { missing, openThroughProgram } = require("./program.js");
const channel = require("./channel.js");
const { ChannelError } = channel;

// How long a client waits for the program it runs to start the engine: the
// program waits up to 30 seconds for the engine to answer its hello, as the
// command line does, and this is that patience with a margin.
const START_PATIENCE_MS = 60000;

// The program the platform package installed for this machine, found as the
// launcher finds it, or undefined.
function installedBinary() {
  const platform = PLATFORMS[platformKey()];
  if (!platform) return undefined;
  const found = findPlatformPackage(path.resolve(__dirname, ".."), platform, packageVersion(), {
    pnp: Boolean(process.versions.pnp),
    resolve: (request) => require.resolve(request),
  });
  return found.binary;
}

// Runs the program's `engine start`, which starts the engine on demand as the
// command line does and waits for its hello. The client starts nothing
// itself. Resolves once an engine answers; rejects with the program's own
// failure, as it wrote it, or `engine_unreachable`.
function startEngine({ binary, env, project }) {
  const program = binary || installedBinary();
  if (!program) {
    return Promise.reject(
      new ChannelError(
        "engine_unreachable",
        "No engine is running, and the `anything2mcp` program this package installs was not found to start one.",
        "Start one with `anything2mcp serve`, or install the package again with its optional dependencies.",
      ),
    );
  }
  const args = ["engine", "start", "--json"];
  if (project !== undefined) args.push("--project", path.resolve(project));
  return new Promise((resolve, reject) => {
    execFile(
      program,
      args,
      { env: { ...process.env, ...env }, timeout: START_PATIENCE_MS, windowsHide: true, maxBuffer: 1024 * 1024 },
      (error, stdout) => {
        if (!error) {
          resolve();
          return;
        }
        let said;
        try {
          said = JSON.parse(stdout);
        } catch {
          said = undefined;
        }
        const failure = said && typeof said.error === "object" && said.error !== null ? said.error : {};
        reject(
          new ChannelError(
            String(failure.code || "engine_unreachable"),
            String(failure.message || "No engine is running, and none was started."),
            String(failure.fix || "Start one with `anything2mcp serve`."),
          ),
        );
      },
    );
  });
}

// Opens the channel to the engine for this operating system. On Windows the
// pipe is opened by the program's `anything2mcp channel` command, which
// checks who serves it as every command does (decision 0466).
async function open(options) {
  const { env = process.env, platform = process.platform, project, name, timeout, binary } = options;
  if (platform === "win32") {
    const program = binary || installedBinary();
    if (!program) throw missing();
    const stream = await openThroughProgram(program, ["channel"], { env: { ...process.env, ...env } });
    return greet(stream, { name, project, timeout });
  }
  const file = channel.socketPath(env, platform);
  channel.checkOwner(file);
  return connectTo(file, { name, project, timeout });
}

// Connects to the local engine that runs as this user. When none answers, it
// runs the installed program's `anything2mcp engine start`, which starts one
// as any command does and waits for it to answer, and then connects to it;
// the client starts nothing by itself. With `start: false` it starts nothing
// and rejects with a ChannelError whose code is `engine_unreachable`.
// `binary` names the program, which is otherwise the one the platform
// package installed; it runs with this process's environment and `env`
// over it. `project` is the folder the engine reads target files from;
// `name` is how this program names itself in the engine's audit log.
//
// On macOS and Linux the client checks that the socket and its directory
// belong to the user before it sends anything. On Windows Node's standard
// library cannot say who serves a named pipe, so the client runs the
// program's `anything2mcp channel` command, the one named by `binary` or
// installed by the platform package, which opens the pipe after the check
// every command makes, that the process serving it runs as this user, and
// then carries the client's bytes and reads none of them.
async function connect(options = {}) {
  const { start = true, binary, env = process.env, project } = options;
  try {
    return await open(options);
  } catch (error) {
    if (!start || !(error instanceof ChannelError) || error.code !== "engine_unreachable") throw error;
  }
  await startEngine({ binary, env, project });
  return open(options);
}

// The version of the package, which is also the version of the engine it matches.
function packageVersion() {
  return JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8")).version;
}

module.exports = {
  ACTIONS,
  Actions,
  ChannelError,
  Client,
  LOCAL_API,
  connect,
  connectTo,
  greet,
  packageVersion,
  socketPath: channel.socketPath,
};
