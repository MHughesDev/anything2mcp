"use strict";

// Run: node --test test/client.test.js
// Tests of the Node client against a fake engine of the test's own. The fake
// listens on a Unix socket where the engine would (on Windows, on a named
// pipe), answers the hello as the engine's channel does, and records each
// request. The last test runs the real binary and is skipped unless
// A2M_TEST_BINARY names one.

const test = require("node:test");
const assert = require("node:assert");
const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

const client = require("../anything2mcp/client/index.js");
const channel = require("../anything2mcp/client/channel.js");
const generator = require("../scripts/generate-actions.js");

const WINDOWS = process.platform === "win32";
const DOCUMENT = path.join(__dirname, "..", "..", "python", "tools", "openapi.json");
const CLIENT_DIR = path.join(__dirname, "..", "anything2mcp", "client");

function scratch() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "a2m-node-"));
}

// A fake engine. `address` is the socket file or pipe name it listens on.
async function fakeEngine({ localApi = "1", dir = scratch(), mode = 0o700 } = {}) {
  const runtime = path.join(dir, "run");
  fs.mkdirSync(runtime, { recursive: true, mode });
  fs.chmodSync(runtime, mode);
  const address = WINDOWS
    ? `\\\\.\\pipe\\a2m-test-${process.pid}-${crypto.randomBytes(6).toString("hex")}`
    : path.join(runtime, "engine.sock");
  const fake = { dir, address, requests: [], connections: 0, answers: new Map(), sockets: new Set(), drops: new Set(), silent: new Set() };
  const server = http.createServer((request, response) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      const body = Buffer.concat(chunks).toString("utf8");
      fake.requests.push({ method: request.method, url: request.url, headers: request.headers, body });
      const send = (status, value, exitClass) => {
        const data = JSON.stringify(value);
        const headers = { "content-type": "application/json", "content-length": Buffer.byteLength(data) };
        if (exitClass) headers["a2m-exit-class"] = exitClass;
        response.writeHead(status, headers);
        response.end(data);
      };
      if (request.url === "/local/hello") {
        if (request.headers["a2m-local-api"] !== localApi) {
          send(400, { ok: false, error: { code: "local_api_mismatch", message: "No.", fix: "Upgrade." } });
          return;
        }
        send(200, { engine_version: "9.9.9", local_api: 1 });
        return;
      }
      const action = request.url.replace("/api/v1/actions/", "");
      if (fake.drops.has(action)) {
        // Takes the request, then goes away without an answer, as an
        // engine that stops does.
        request.socket.destroy();
        return;
      }
      // Takes the request and is still carrying it out: no answer yet.
      if (fake.silent.has(action)) return;
      const [status, answer, exitClass] = fake.answers.get(action) || [
        200,
        { ok: true, action, result: { echoed: body ? JSON.parse(body) : null }, warnings: [] },
        "succeeded",
      ];
      send(status, answer, exitClass);
    });
  });
  server.on("connection", (socket) => {
    fake.connections += 1;
    fake.sockets.add(socket);
    socket.on("close", () => fake.sockets.delete(socket));
  });
  await new Promise((resolve) => server.listen(address, resolve));
  fake.env = { A2M_HOME: dir };
  fake.stop = async () => {
    for (const socket of fake.sockets) socket.destroy();
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(dir, { recursive: true, force: true });
  };
  return fake;
}

test("it says hello first, then posts the action and returns the engine's answer", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const c = await client.connectTo(engine.address, { name: "tests", project: engine.dir });
  t.after(() => c.close());
  assert.strictEqual(c.engineVersion, "9.9.9");
  const answer = await c.call("engine.status", { a: 1 }, { idempotencyKey: "example-key-1" });
  assert.strictEqual(answer.ok, true);
  assert.strictEqual(answer.exitClass, "succeeded");
  assert.deepStrictEqual(answer.result, { echoed: { a: 1 } });
  const [hello, post] = engine.requests;
  assert.strictEqual(hello.url, "/local/hello");
  assert.strictEqual(hello.headers["a2m-local-api"], "1");
  assert.strictEqual(hello.headers["a2m-client-label"], "sdk:tests");
  assert.match(hello.headers["a2m-client-version"], /^\d+\.\d+\.\d+/);
  assert.strictEqual(post.method, "POST");
  assert.strictEqual(post.url, "/api/v1/actions/engine.status");
  assert.strictEqual(post.headers["idempotency-key"], "example-key-1");
  assert.strictEqual(post.headers["a2m-project"], require("../anything2mcp/client/client.js").encodePath(path.resolve(engine.dir)));
  assert.ok(!/[^A-Za-z0-9\-._~/%]/.test(post.headers["a2m-project"]), "the project path is percent-encoded");
});

test("every call goes over the one connection that said hello", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  await c.call("engine.status");
  await c.engineStatus();
  await c.toolSearch({ query: "x" });
  assert.strictEqual(engine.connections, 1);
  assert.strictEqual(engine.requests.filter((r) => r.url === "/local/hello").length, 1);
});

test("a typed method posts to its action's route with its input and nothing else", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  const answer = await c.toolCall({ tool: "example_order_get", input: { order_id: "A-100" } });
  assert.strictEqual(answer.action, "tool.call");
  const post = engine.requests.at(-1);
  assert.strictEqual(post.url, "/api/v1/actions/tool.call");
  assert.deepStrictEqual(JSON.parse(post.body), { tool: "example_order_get", input: { order_id: "A-100" } });
  await c.engineStatus();
  assert.deepStrictEqual(JSON.parse(engine.requests.at(-1).body), {});
  assert.strictEqual(engine.requests.at(-1).url, "/api/v1/actions/engine.status");
});

test("a failure is returned as the engine wrote it and not thrown", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  engine.answers.set("tool.call", [
    200,
    { ok: false, action: "tool.call", error: { class: "policy", code: "not_granted", message: "No.", fix: "Ask.", retry_safe: false } },
    "failed",
  ]);
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  const answer = await c.toolCall({ tool: "x" });
  assert.strictEqual(answer.ok, false);
  assert.strictEqual(answer.exitClass, "failed");
  assert.deepStrictEqual(answer.error, { failureClass: "policy", code: "not_granted", message: "No.", fix: "Ask.", retrySafe: false });
});

test("an answer that names no known exit class is not a success", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  engine.answers.set("engine.status", [200, { ok: true, action: "engine.status" }, "surprise"]);
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  assert.strictEqual((await c.engineStatus()).exitClass, "failed");
});

test("an unreadable answer is a named failure", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  engine.answers.set("engine.status", [200, "not an object", "succeeded"]);
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  await assert.rejects(c.engineStatus(), (error) => error.code === "answer_lost" && error.retrySafe === false);
});

test("a call whose answer is lost after the engine took it is not safe to make again", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  engine.drops.add("tool.call");
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  await assert.rejects(c.toolCall({ tool: "example_order_update", input: {} }), (error) => {
    assert.strictEqual(error.code, "answer_lost");
    assert.strictEqual(error.retrySafe, false);
    assert.ok(error.fix.startsWith("Do not make the call again"), error.fix);
    assert.ok(!error.fix.includes("Run it again"), error.fix);
    return true;
  });
  assert.strictEqual(engine.requests.at(-1).url, "/api/v1/actions/tool.call");
  // The client's own timeout leaves the call in doubt too: the engine may
  // still be carrying it out.
  engine.drops.clear();
  engine.silent.add("tool.call");
  const slow = await client.connectTo(engine.address, { timeout: 200 });
  t.after(() => slow.close());
  await assert.rejects(
    slow.toolCall({ tool: "example_order_update", input: {} }),
    (error) => error.code === "answer_lost" && error.retrySafe === false,
  );
});

test("an action's name that is not noun.verb is refused before anything is sent", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  const sent = engine.requests.length;
  for (const name of ["../local/hello", "tool.call?x=1", "tool.call#x", "tool%2Ecall", "Tool.Call", "tool", 7]) {
    await assert.rejects(c.call(name), TypeError);
  }
  assert.strictEqual(engine.requests.length, sent);
});

test("a lost connection is named, and the client does not reconnect without a hello", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const c = await client.connectTo(engine.address);
  const closed = new Promise((resolve) => c._socket.once("close", resolve));
  for (const socket of engine.sockets) socket.destroy();
  await closed;
  await assert.rejects(c.engineStatus(), (error) => error.code === "connection_lost" && error.retrySafe === true);
  await assert.rejects(c.engineStatus(), (error) => error.code === "connection_lost");
  assert.strictEqual(engine.connections, 1);
});

test("a refused hello is a named failure", async (t) => {
  const engine = await fakeEngine({ localApi: "2" });
  t.after(() => engine.stop());
  await assert.rejects(client.connectTo(engine.address), (error) => error.code === "local_api_mismatch");
});

test("no engine is a named failure", async () => {
  const address = WINDOWS
    ? `\\\\.\\pipe\\a2m-test-none-${process.pid}`
    : path.join(scratch(), "none.sock");
  await assert.rejects(client.connectTo(address), (error) => error.code === "engine_unreachable");
});

test("a check of the far end that does not pass stops the client before the hello", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  await assert.rejects(client.connectTo(engine.address, { verify: () => false }), (error) => error.code === "peer_not_allowed");
  assert.strictEqual(engine.requests.length, 0, "nothing was sent");
  const c = await client.connectTo(engine.address, { verify: async () => true });
  c.close();
});

test("on macOS and Linux a socket in a directory others can change is not trusted", { skip: WINDOWS }, async (t) => {
  const engine = await fakeEngine({ mode: 0o777 });
  t.after(() => engine.stop());
  await assert.rejects(client.connect({ env: engine.env }), (error) => error.code === "peer_not_allowed");
  assert.strictEqual(engine.requests.length, 0);
});

test("on macOS and Linux connect finds the socket where the engine puts it", { skip: WINDOWS }, async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const c = await client.connect({ env: engine.env, name: "tests" });
  t.after(() => c.close());
  assert.strictEqual((await c.engineStatus()).ok, true);
  await assert.rejects(client.connect({ env: { A2M_HOME: path.join(scratch(), "nothing") } }), (error) => error.code === "engine_unreachable");
});

test("each system keeps the socket where the engine puts it", () => {
  const linux = { HOME: "/home/pat" };
  assert.strictEqual(client.socketPath(linux, "linux"), "/home/pat/.local/share/anything2mcp/run/engine.sock");
  assert.strictEqual(client.socketPath({ ...linux, XDG_RUNTIME_DIR: "/run/user/1000" }, "linux"), "/run/user/1000/anything2mcp/engine.sock");
  assert.strictEqual(client.socketPath({ ...linux, XDG_RUNTIME_DIR: "relative" }, "linux"), "/home/pat/.local/share/anything2mcp/run/engine.sock");
  assert.strictEqual(client.socketPath({ HOME: "/Users/pat", TMPDIR: "/var/folders/x/T" }, "darwin"), "/var/folders/x/T/anything2mcp/engine.sock");
  assert.strictEqual(client.socketPath({ A2M_HOME: "/srv/a2m", HOME: "/ignored" }, "linux"), "/srv/a2m/run/engine.sock");
  assert.throws(() => client.socketPath({ A2M_HOME: "relative" }, "linux"), (error) => error.code === "path_not_allowed");
});

test("the pipe is named as the engine names it, from the runtime directory's text", () => {
  const runtime = "C:\\Users\\pat\\AppData\\Local\\anything2mcp\\run";
  const sid = "S-1-5-21-1111111111-2222222222-3333333333-1001";
  assert.strictEqual(client.windowsRuntimeDir({ LOCALAPPDATA: "C:\\Users\\pat\\AppData\\Local" }), runtime);
  assert.strictEqual(client.windowsRuntimeDir({ A2M_HOME: "D:\\a2m\\" }), "D:\\a2m\\run");
  assert.strictEqual(client.windowsRuntimeDir({ A2M_HOME: "D:/a2m" }), "D:/a2m\\run");
  assert.throws(() => client.windowsRuntimeDir({ A2M_HOME: "relative" }), (error) => error.code === "path_not_allowed");
  assert.throws(() => client.windowsRuntimeDir({}), (error) => error.code === "path_not_allowed");
  // The tag is the same one the Python client's test holds to the engine.
  assert.strictEqual(client.pipeName(runtime, sid), `\\\\.\\pipe\\anything2mcp.${sid}.8af282852451f074`);
});

test("on Windows the client refuses to connect until the far end can be checked", async () => {
  // Node's standard library cannot say who serves a pipe; nothing is opened without a check.
  await assert.rejects(
    client.connect({ platform: "win32", env: { LOCALAPPDATA: "C:\\Users\\pat\\AppData\\Local" } }),
    (error) => error.code === "client_not_built",
  );
});

test("the generated methods are what the generator writes from the saved document", () => {
  const result = spawnSync(process.execPath, [path.join(__dirname, "..", "scripts", "generate-actions.js"), "--check"], { encoding: "utf8" });
  assert.strictEqual(result.status, 0, result.stderr);
});

test("every action of the document has a method, and a type, on the client", () => {
  const document = JSON.parse(fs.readFileSync(DOCUMENT, "utf8"));
  const actions = generator.actionsOf(document).map(([action]) => action);
  assert.deepStrictEqual([...client.ACTIONS], actions);
  assert.ok(actions.length > 80);
  const declared = fs.readFileSync(path.join(CLIENT_DIR, "actions.d.ts"), "utf8");
  for (const action of actions) {
    const name = generator.methodName(action);
    assert.strictEqual(typeof client.Client.prototype[name], "function", action);
    assert.ok(declared.includes(`  ${name}(input`), `${name} is declared`);
  }
});

test("a generator that does not understand the document stops", () => {
  const free = { info: {}, paths: { "/api/v1/actions/a.b": { post: { "x-action": "a.b", summary: "S.", requestBody: { content: { "application/json": { schema: { type: "object", additionalProperties: {}, properties: {} } } } } } } } };
  assert.throws(() => generator.render(free), generator.GenerateError);
  assert.throws(() => generator.render({ info: {}, paths: {} }), generator.GenerateError);
  assert.strictEqual(generator.methodName("tool.call"), "toolCall");
  assert.strictEqual(generator.methodName("run.get"), "runGet");
});

const BINARY = process.env.A2M_TEST_BINARY || "";
test("the client reaches a real engine and the engine answers", { skip: !BINARY || WINDOWS }, async (t) => {
  const home = scratch();
  const env = { ...process.env, A2M_HOME: home, A2M_VAULT_PASSPHRASE: "secret-for-tests-only" };
  const engine = spawn(BINARY, ["serve"], { env, stdio: ["ignore", "ignore", "pipe"] });
  t.after(() => {
    engine.kill();
    fs.rmSync(home, { recursive: true, force: true });
  });
  const socket = path.join(home, "run", "engine.sock");
  // Waits on the named condition, the socket existing, up to a deadline.
  const deadline = Date.now() + 60000;
  while (!fs.existsSync(socket)) {
    assert.ok(engine.exitCode === null, "the engine stopped");
    assert.ok(Date.now() < deadline, "the engine did not open its channel within a minute");
    await new Promise((resolve) => setImmediate(resolve));
  }
  const c = await client.connect({ env: { A2M_HOME: home }, name: "tests" });
  t.after(() => c.close());
  const status = await c.engineStatus();
  assert.strictEqual(status.ok, true, JSON.stringify(status.body));
  assert.strictEqual(status.exitClass, "succeeded");
  const unknown = await c.call("no.such");
  assert.strictEqual(unknown.ok, false);
});
