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
// engine's response as it was written. It does not start the engine. The
// methods of `Actions` are generated from the engine's OpenAPI document.

const fs = require("node:fs");
const path = require("node:path");

const { ACTIONS, Actions } = require("./actions.js");
const { Client, LOCAL_API, connectTo } = require("./client.js");
const channel = require("./channel.js");
const { ChannelError } = channel;

// Connects to the local engine that runs as this user. It starts nothing:
// when no engine answers it rejects with a ChannelError whose code is
// `engine_unreachable`. `project` is the folder the engine reads target
// files from; `name` is how this program names itself in the engine's audit
// log.
//
// On macOS and Linux the client checks that the socket and its directory
// belong to the user before it sends anything. On Windows Node's standard
// library cannot say who serves a named pipe, so the client cannot make the
// check by itself and refuses to connect (`client_not_built`) unless the
// caller supplies `verifyPipeServer`, a function of the pipe's socket that
// resolves to true only when the process serving it runs as this user. The
// pipe's name needs the user's security identifier: `sid`.
async function connect(options = {}) {
  const { env = process.env, platform = process.platform, project, name, timeout, verifyPipeServer, sid } = options;
  if (platform === "win32") {
    if (typeof verifyPipeServer !== "function" || typeof sid !== "string") {
      throw new ChannelError(
        "client_not_built",
        "This client cannot yet check who serves the engine's pipe on Windows.",
        "Use the `anything2mcp` command on Windows for now, or pass `verifyPipeServer` and `sid`.",
      );
    }
    const address = channel.pipeName(channel.windowsRuntimeDir(env), sid);
    return connectTo(address, { verify: verifyPipeServer, name, project, timeout });
  }
  const file = channel.socketPath(env, platform);
  channel.checkOwner(file);
  return connectTo(file, { name, project, timeout });
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
  packageVersion,
  pipeName: channel.pipeName,
  socketPath: channel.socketPath,
  windowsRuntimeDir: channel.windowsRuntimeDir,
};
