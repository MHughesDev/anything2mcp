"use strict";

// The local channel through the program's `anything2mcp channel` command, for
// Windows. Node's standard library cannot say who serves a named pipe, so the
// client does not open the pipe itself: the program opens it with the connect
// every command makes, which checks that the process serving the pipe runs as
// this user before anything is sent, and then copies bytes between the pipe
// and its standard input and output, reading none of them (decision 0999).
// The client speaks HTTP over those bytes exactly as it does over a socket:
// its own hello first, then its requests.
//
// The command says one line on standard error: `OPEN_LINE` once the checked
// connection is made, or the refusal as one JSON object, after which it ends.

const { spawn: spawnProcess } = require("node:child_process");
const { Duplex } = require("node:stream");

const { ChannelError, lost } = require("./channel.js");

const OPEN_LINE = "anything2mcp: the local channel is open.";
// How long the client waits for the command to say it is open: the command
// waits up to 30 seconds for a pipe whose every instance is taken, as any
// command does, and this is that patience with a margin.
const OPEN_PATIENCE_MS = 60000;
// The longest line of standard error read before the command has said it is open.
const LINE_LIMIT = 64 * 1024;

// The child's standard input and output as one stream, which the client and
// Node's HTTP agent use as they use a socket.
class ProgramStream extends Duplex {
  constructor(child) {
    super();
    this._child = child;
    this._idle = 0;
    this._timer = null;
    child.stdout.on("data", (chunk) => {
      this._touch();
      if (!this.push(chunk)) child.stdout.pause();
    });
    child.stdout.on("end", () => this.push(null));
    child.stdin.on("error", (error) => this.destroy(error));
    // The command has ended, and with it the connection.
    child.on("close", () => this.destroy());
  }

  _read() {
    this._child.stdout.resume();
  }

  _write(chunk, encoding, callback) {
    this._touch();
    this._child.stdin.write(chunk, encoding, callback);
  }

  _final(callback) {
    this._child.stdin.end(callback);
  }

  _destroy(error, callback) {
    clearTimeout(this._timer);
    if (this._child.exitCode === null && this._child.signalCode === null) this._child.kill();
    callback(error);
  }

  // As a socket's: `timeout` once nothing has moved for `ms` milliseconds;
  // zero turns it off.
  setTimeout(ms, listener) {
    this._idle = ms;
    if (listener) this.once("timeout", listener);
    this._touch();
    return this;
  }

  _touch() {
    clearTimeout(this._timer);
    this._timer = null;
    if (this._idle > 0 && !this.destroyed) {
      this._timer = setTimeout(() => this.emit("timeout"), this._idle);
      this._timer.unref();
    }
  }

  setNoDelay() {
    return this;
  }

  setKeepAlive() {
    return this;
  }

  // As a socket's: an idle connection does not keep the process alive.
  ref() {
    for (const part of [this._child, this._child.stdin, this._child.stdout, this._child.stderr]) part?.ref?.();
    return this;
  }

  unref() {
    for (const part of [this._child, this._child.stdin, this._child.stdout, this._child.stderr]) part?.unref?.();
    return this;
  }
}

// The refusal the command wrote, as a ChannelError, or null when the line is
// not one.
function refusalOf(line) {
  let said;
  try {
    said = JSON.parse(line);
  } catch {
    return null;
  }
  const error = said && typeof said === "object" && said.error && typeof said.error === "object" ? said.error : null;
  if (error === null || typeof error.code !== "string" || error.code === "") return null;
  return new ChannelError(error.code, String(error.message ?? ""), String(error.fix ?? ""));
}

// Runs `program` with `args` (`["channel"]` for the product's program) and
// resolves to the stream once it says the checked connection is made.
// Rejects with the command's own refusal, such as `engine_unreachable` or
// `peer_not_allowed`, or with `connection_lost` when it ends or stops
// answering without saying either; nothing has then reached an engine.
function openThroughProgram(program, args, { env = process.env, spawn = spawnProcess, patience = OPEN_PATIENCE_MS } = {}) {
  return new Promise((resolve, reject) => {
    let child;
    try {
      child = spawn(program, args, { env, stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
    } catch {
      reject(missing());
      return;
    }
    let settled = false;
    let said = "";
    const settle = (error, stream) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) {
        child.kill();
        reject(error);
      } else {
        resolve(stream);
      }
    };
    const timer = setTimeout(
      () => settle(lost("The `anything2mcp channel` command did not open the engine's channel in time, so nothing was sent.")),
      patience,
    );
    child.once("error", (error) => {
      settle(error && error.code === "ENOENT" ? missing() : lost("The `anything2mcp channel` command could not be run, so nothing was sent."));
    });
    child.once("exit", () => {
      // A refusal the command wrote is read whole before it is told.
      setImmediate(() => {
        settle(refusalOf(said.trim()) || lost("The `anything2mcp channel` command ended before it opened the engine's channel, so nothing was sent."));
      });
    });
    child.stderr.on("data", (chunk) => {
      if (settled) return;
      said += chunk.toString("utf8");
      const end = said.indexOf("\n");
      if (end === -1) {
        if (said.length > LINE_LIMIT) settle(lost("The `anything2mcp channel` command said what it should not, so nothing was sent."));
        return;
      }
      const line = said.slice(0, end).replace(/\r$/, "");
      if (line === OPEN_LINE) {
        settle(null, new ProgramStream(child));
        return;
      }
      const refusal = refusalOf(line);
      if (refusal !== null) settle(refusal);
      // Anything else is waited out: the command's exit tells it.
    });
  });
}

function missing() {
  return new ChannelError(
    "engine_unreachable",
    "The `anything2mcp` program this package installs was not found, and on Windows the client reaches the engine through it.",
    "Install the package again with its optional dependencies, or pass `binary` with the program's path.",
  );
}

module.exports = { OPEN_LINE, ProgramStream, missing, openThroughProgram };
