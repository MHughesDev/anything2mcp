"use strict";

// The client: sends one action to the local engine and returns its answer.
// It holds no engine logic. It says hello once on its connection, posts the
// action's input to the action's REST route, and hands back the response the
// engine wrote, without branching on what it means. The hello belongs to a
// connection, so the client keeps the one connection it said hello on and,
// when that is lost, says so; it never reconnects behind the caller's back.

const http = require("node:http");
const net = require("node:net");
const path = require("node:path");

const { Actions } = require("./actions.js");
const channel = require("./channel.js");
const { ChannelError } = channel;

// The local API version this client speaks; the engine refuses another.
const LOCAL_API = 1;
const REST_PREFIX = "/api/v1";
// The longest answer read, in bytes.
const ANSWER_LIMIT = 16 * 1024 * 1024;
const EXIT_CLASSES = ["succeeded", "failed", "malformed", "person_needed"];

function version() {
  try {
    return require("../package.json").version;
  } catch {
    return "0.0.0";
  }
}

// A folder's path as the project header carries it: the same
// percent-encoding the command line uses.
function encodePath(text) {
  let out = "";
  for (const byte of Buffer.from(text, "utf8")) {
    const char = String.fromCharCode(byte);
    out += /[A-Za-z0-9\-._~/]/.test(char) ? char : `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return out;
}

function toError(error) {
  if (!error || typeof error !== "object") return null;
  return {
    failureClass: String(error.class ?? ""),
    code: String(error.code ?? ""),
    message: String(error.message ?? ""),
    fix: String(error.fix ?? ""),
    retrySafe: Boolean(error.retry_safe),
  };
}

function toResponse(body, exitHeader) {
  return {
    ok: Boolean(body.ok),
    action: String(body.action ?? ""),
    // The header, as the command line reads it; an answer that names no
    // known class is not one to call a success.
    exitClass: EXIT_CLASSES.includes(exitHeader) ? exitHeader : "failed",
    result: body.result,
    error: toError(body.error),
    warnings: Array.isArray(body.warnings) ? body.warnings : [],
    next: body.next,
    body,
  };
}

class Client extends Actions {
  // Use `connect`. `socket` is an open connection to the engine.
  constructor(socket, { name = "node", project, timeout } = {}) {
    super();
    this._socket = socket;
    this._label = `sdk:${name}`;
    this._project = project === undefined ? null : path.resolve(project);
    this._timeout = timeout;
    this._closed = false;
    this.engineVersion = "";
    // One connection, kept: the engine's hello holds for that connection only.
    const connection = socket;
    this._agent = new (class extends http.Agent {
      createConnection() {
        return connection;
      }
    })({ keepAlive: true, maxSockets: 1 });
    socket.on("close", () => {
      this._closed = true;
    });
    socket.on("error", () => {});
  }

  _exchange(method, requestPath, headers, payload) {
    return new Promise((resolve, reject) => {
      if (this._closed) {
        reject(channel.lost());
        return;
      }
      const request = http.request(
        { method, path: requestPath, headers, agent: this._agent, host: "localhost" },
        (answer) => {
          const chunks = [];
          let size = 0;
          answer.on("data", (chunk) => {
            size += chunk.length;
            if (size > ANSWER_LIMIT) {
              reject(channel.lost("The engine's answer was too long."));
              this._socket.destroy();
              return;
            }
            chunks.push(chunk);
          });
          answer.on("end", () => {
            let body = null;
            try {
              body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
            } catch {
              body = null;
            }
            if (body === null || typeof body !== "object" || Array.isArray(body)) {
              reject(channel.lost("The engine's answer could not be read."));
              return;
            }
            resolve({ status: answer.statusCode, body, exit: answer.headers["a2m-exit-class"] });
          });
          answer.on("error", () => reject(channel.lost()));
        },
      );
      if (this._timeout) {
        request.setTimeout(this._timeout, () => {
          request.destroy();
          this._socket.destroy();
        });
      }
      request.on("error", () => reject(channel.lost()));
      request.end(payload);
    });
  }

  async _hello() {
    const { status, body } = await this._exchange("GET", "/local/hello", {
      "a2m-client-version": version(),
      "a2m-local-api": String(LOCAL_API),
      "a2m-client-label": this._label,
    });
    if (status !== 200) {
      const error = body.error || {};
      throw new ChannelError(
        String(error.code ?? "hello_invalid"),
        String(error.message ?? "The engine refused the hello."),
        String(error.fix ?? ""),
      );
    }
    this.engineVersion = String(body.engine_version ?? "");
  }

  // Sends one action, such as "engine.status" or "tool.call", with its
  // input, and resolves to the engine's response, success or not. It
  // rejects with a ChannelError only when the engine was not reached or its
  // answer could not be read.
  async call(action, input = {}, options = {}) {
    const headers = { "content-type": "application/json" };
    if (this._project !== null) headers["a2m-project"] = encodePath(this._project);
    if (options.idempotencyKey !== undefined) headers["idempotency-key"] = options.idempotencyKey;
    const payload = JSON.stringify(input ?? {});
    headers["content-length"] = String(Buffer.byteLength(payload));
    const { body, exit } = await this._exchange("POST", `${REST_PREFIX}/actions/${action}`, headers, payload);
    return toResponse(body, exit);
  }

  close() {
    this._closed = true;
    this._agent.destroy();
    this._socket.destroy();
  }
}

// Opens a connection to `address` (a socket file, or a pipe's name), runs the
// caller's check of the far end if there is one, and says hello. This is
// the one place a transport is opened; `connect` in index.js chooses the
// address and the check for the operating system.
async function connectTo(address, { verify, name, project, timeout } = {}) {
  const socket = await new Promise((resolve, reject) => {
    const opened = net.createConnection({ path: address });
    opened.once("connect", () => {
      opened.removeListener("error", reject);
      resolve(opened);
    });
    opened.once("error", reject);
  }).catch((error) => {
    if (error && (error.code === "ENOENT" || error.code === "ECONNREFUSED")) throw channel.notRunning();
    throw channel.lost("The engine's channel could not be opened.");
  });
  try {
    if (verify !== undefined && (await verify(socket)) !== true) {
      throw new ChannelError(
        "peer_not_allowed",
        "What serves the engine's channel does not belong to you.",
        "Stop the program that made it, and start the engine again.",
      );
    }
    const client = new Client(socket, { name, project, timeout });
    await client._hello();
    return client;
  } catch (error) {
    socket.destroy();
    throw error;
  }
}

module.exports = { ANSWER_LIMIT, Client, LOCAL_API, REST_PREFIX, connectTo, encodePath, toResponse };
