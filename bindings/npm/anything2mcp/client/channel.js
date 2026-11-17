"use strict";

// Where the local engine's channel is, and the checks a client makes of it
// before it sends anything. The rules for the place are the engine's own
// (`Paths` in the `a2m-actions` crate and the channel of `a2m-local`),
// restated here because a client must find the channel before it can ask
// anything; a test holds them to the engine's answer when a built binary is
// given. On macOS and Linux the channel is a Unix socket. On Windows it is
// a named pipe, whose name carries the user's security identifier and a
// digest of the runtime directory.

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const APP_FOLDER = "anything2mcp";
const HOME_VARIABLE = "A2M_HOME";
const SOCKET_NAME = "engine.sock";

// The engine's failure code, a message, and the fix; every way the engine
// was not reached is one of these.
class ChannelError extends Error {
  constructor(code, message, fix) {
    super(`${message} ${fix}`.trim());
    this.name = "ChannelError";
    this.code = code;
    this.reason = message;
    this.fix = fix;
  }
}

function notRunning() {
  return new ChannelError(
    "engine_unreachable",
    "No engine is running.",
    "Start one with `anything2mcp serve`, or run any `anything2mcp` command.",
  );
}

function lost(why = "The connection to the engine was lost.") {
  return new ChannelError("connection_lost", why, "Run it again.");
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

// The runtime directory on Windows, as text, by `Paths::resolve`. The pipe's
// tag is a digest of this text exactly as the engine holds it.
function windowsRuntimeDir(env = process.env) {
  // As Rust's `Path::join` does on Windows: add a backslash unless the text
  // already ends in a separator, and change nothing else. `path.win32.join`
  // would normalise the text, and the digest is of the text as it is.
  const win = {
    join: (base, ...parts) =>
      parts.reduce((text, part) => (/[\\/]$/.test(text) ? text + part : `${text}\\${part}`), base),
  };
  const absolute = (value) => /^(?:[A-Za-z]:[\\/]|[\\/]{2}[^\\/]+[\\/]+[^\\/]+)/.test(value);
  const home = env[HOME_VARIABLE];
  if (home) {
    if (!absolute(home)) {
      throw new ChannelError(
        "path_not_allowed",
        `\`${HOME_VARIABLE}\` is not an absolute path.`,
        `Set \`${HOME_VARIABLE}\` to the full path of a directory.`,
      );
    }
    return win.join(home, "run");
  }
  const local = env.LOCALAPPDATA;
  if (!local || !absolute(local)) {
    throw new ChannelError(
      "path_not_allowed",
      "The local application data directory is not known.",
      `Set \`${HOME_VARIABLE}\` to the full path of a directory for the engine.`,
    );
  }
  return win.join(local, APP_FOLDER, "run");
}

// The pipe's name, as `Place::new` in `a2m-local` makes it: the user's
// security identifier, and the first 16 hex digits of the SHA-256 of the
// runtime directory's text.
function pipeName(runtimeDirText, sid) {
  const tag = crypto.createHash("sha256").update(runtimeDirText, "utf8").digest("hex").slice(0, 16);
  return `\\\\.\\pipe\\anything2mcp.${sid}.${tag}`;
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
  checkOwner,
  lost,
  notRunning,
  pipeName,
  runtimeDir,
  socketPath,
  windowsRuntimeDir,
};
