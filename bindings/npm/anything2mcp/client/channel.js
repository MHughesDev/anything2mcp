"use strict";

// Where the local engine's channel is, and the checks a client makes of it
// before it sends anything. The rules for the place are the engine's own
// (`Paths` in the `a2m-actions` crate and the channel of `a2m-local`),
// restated here because a client must find the channel before it can ask
// anything; a test holds them to the engine's answer when a built binary is
// given. On macOS and Linux the channel is a Unix socket. On Windows it is
// a named pipe, which the client reaches through the program's
// `anything2mcp channel` command (program.js), so its name is not made here.

const fs = require("node:fs");
const path = require("node:path");

const APP_FOLDER = "anything2mcp";
const HOME_VARIABLE = "A2M_HOME";
const SOCKET_NAME = "engine.sock";

// The failure code, a message, and the fix; every way the engine was not
// reached, or its answer was lost, is one of these. `retrySafe` is true when
// nothing was sent, so the call may be made again. It is false for
// `answer_lost`: the engine may have had the call, which may have started a
// run that writes, and a second call may write a second time.
class ChannelError extends Error {
  constructor(code, message, fix, retrySafe = true) {
    super(`${message} ${fix}`.trim());
    this.name = "ChannelError";
    this.code = code;
    this.reason = message;
    this.fix = fix;
    this.retrySafe = retrySafe;
  }
}

function notRunning() {
  return new ChannelError(
    "engine_unreachable",
    "No engine is running.",
    "Start one with `anything2mcp serve`, or run any `anything2mcp` command.",
  );
}

// The connection is closed, or was lost before the request was sent:
// nothing reached the engine.
function lost(why = "The connection to the engine is closed, so nothing was sent.") {
  return new ChannelError("connection_lost", why, "Connect again with `connect()`, then make the call again.");
}

// The request may have reached the engine, and its answer is lost. `call`
// says what was sent: whether it carried an idempotency key, which decides
// the advice, and the action, input, and time (milliseconds) that name the
// run list to look in when it did not.
function answerLost(why = "The connection to the engine was lost before it answered.", call = {}) {
  const { keyed = false, action = "", input = {}, sentAt = 0 } = call;
  let fix;
  if (keyed) {
    fix =
      "Make the same call again with the same idempotency key and nothing changed: it gives back " +
      "the run the first call started, finished or not, and starts no other.";
  } else {
    const since = Math.max(sentAt - 60000, 0);
    const tool = action === "tool.call" && input && typeof input.tool === "string" ? input.tool : null;
    const look = tool === null ? `client.runList({ since: ${since} })` : `client.runList({ since: ${since}, tool: ${JSON.stringify(tool)} })`;
    fix =
      "Do not make the call again: a second call may start a second run and write twice. " +
      `Find the run first with the run list, \`${look}\`, since one minute before the call, ` +
      "and do what that run's `next` says; and pass `idempotencyKey` in the options of a call that may write, " +
      "so that it can be made again.";
  }
  return new ChannelError(
    "answer_lost",
    `${why} The engine may have had the call: it may have started a run, and that run may have written.`,
    fix,
    false,
  );
}

// A directory taken from the environment: set, and absolute on this system.
function absoluteDir(env, name, flavor) {
  const value = env[name];
  return value && flavor.isAbsolute(value) ? value : null;
}

// The engine's runtime directory on macOS and Linux, by the engine's rules.
function runtimeDir(env = process.env, platform = process.platform) {
  const posix = path.posix;
  const home = env[HOME_VARIABLE];
  if (home) {
    if (!posix.isAbsolute(home)) {
      throw new ChannelError(
        "path_not_allowed",
        `\`${HOME_VARIABLE}\` is not an absolute path.`,
        `Set \`${HOME_VARIABLE}\` to the full path of a directory.`,
      );
    }
    return posix.join(home, "run");
  }
  const userHome = absoluteDir(env, "HOME", posix);
  if (userHome === null) {
    throw new ChannelError(
      "path_not_allowed",
      "The home directory is not known.",
      `Set \`${HOME_VARIABLE}\` to the full path of a directory for the engine.`,
    );
  }
  if (platform === "darwin") {
    const tmp = absoluteDir(env, "TMPDIR", posix);
    if (tmp !== null) return posix.join(tmp, APP_FOLDER);
    return posix.join(userHome, "Library", "Application Support", APP_FOLDER, "run");
  }
  const run = absoluteDir(env, "XDG_RUNTIME_DIR", posix);
  if (run !== null) return posix.join(run, APP_FOLDER);
  const data = absoluteDir(env, "XDG_DATA_HOME", posix) || posix.join(userHome, ".local", "share");
  return posix.join(data, APP_FOLDER, "run");
}

function socketPath(env = process.env, platform = process.platform) {
  return path.posix.join(runtimeDir(env, platform), SOCKET_NAME);
}

// The client's check of the engine on macOS and Linux: the socket and its
// directory are this user's, and nobody else can change the directory.
// Without it another program could make the socket first and answer in the
// engine's place.
function checkOwner(socketFile) {
  let folder;
  let found;
  try {
    folder = fs.lstatSync(path.dirname(socketFile));
    found = fs.lstatSync(socketFile);
  } catch (error) {
    if (error && error.code === "ENOENT") throw notRunning();
    throw error;
  }
  const me = process.getuid();
  const ours =
    folder.isDirectory() &&
    folder.uid === me &&
    (folder.mode & 0o077) === 0 &&
    found.isSocket() &&
    found.uid === me;
  if (!ours) {
    throw new ChannelError(
      "peer_not_allowed",
      "What listens at the engine's place does not belong to you.",
      "Check who owns the engine's runtime directory, remove what is not yours to trust, and start the engine again.",
    );
  }
}

module.exports = {
  APP_FOLDER,
  ChannelError,
  answerLost,
  checkOwner,
  lost,
  notRunning,
  runtimeDir,
  socketPath,
};
