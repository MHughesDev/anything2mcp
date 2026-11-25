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
const program = require("../anything2mcp/client/program.js");
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
    assert.match(error.fix, /`client\.runList\(\{ since: \d+, tool: "example_order_update" \}\)`/);
    assert.ok(error.fix.includes("one minute before the call"), error.fix);
    assert.ok(error.fix.includes("what that run's `next` says"), error.fix);
    assert.ok(error.fix.includes("pass `idempotencyKey` in the options of a call that may write"), error.fix);
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

test("a keyed call whose answer is lost is told to make the same call again", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  engine.drops.add("tool.call");
  const c = await client.connectTo(engine.address);
  t.after(() => c.close());
  await assert.rejects(c.toolCall({ tool: "example_order_update", input: {} }, { idempotencyKey: "example-key-1" }), (error) => {
    assert.strictEqual(error.code, "answer_lost");
    assert.strictEqual(error.retrySafe, false);
    assert.strictEqual(
      error.fix,
      "Make the same call again with the same idempotency key and nothing changed: it gives back " +
        "the run the first call started, finished or not, and starts no other.",
    );
    return true;
  });
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
  const nothing = { A2M_HOME: path.join(scratch(), "nothing") };
  await assert.rejects(client.connect({ env: nothing, start: false }), (error) => error.code === "engine_unreachable");
  // Asked to start one with a program that cannot be run, the same.
  await assert.rejects(
    client.connect({ env: nothing, binary: path.join(scratch(), "no-such-program") }),
    (error) => error.code === "engine_unreachable",
  );
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

// A stand-in for the program's `anything2mcp channel` command, run by Node
// itself so that it runs on every system: it says it is open and then copies
// bytes between its standard streams and the fake engine, or writes a
// refusal and ends, as the command does.
const FAKE_PROGRAM = `
const net = require("node:net");
const [, , mode, address] = process.argv;
if (mode === "refuse") {
  process.stderr.write(JSON.stringify({ ok: false, action: "channel", error: { class: "policy", code: "peer_not_allowed", message: "Not yours.", fix: "Stop it." } }) + "\\n");
  process.exit(1);
}
if (mode === "silent") process.exit(3);
const socket = net.createConnection({ path: address }, () => {
  process.stderr.write("anything2mcp: the local channel is open.\\n");
  process.stdin.pipe(socket);
  socket.pipe(process.stdout);
});
socket.on("close", () => process.exit(0));
`;

function fakeProgram() {
  const file = path.join(scratch(), "fake-channel.js");
  fs.writeFileSync(file, FAKE_PROGRAM);
  return file;
}

test("through the program's channel command the client says hello, calls, and is answered", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  const stream = await program.openThroughProgram(process.execPath, [fakeProgram(), "open", engine.address]);
  const c = await client.greet(stream, { name: "tests" });
  t.after(() => c.close());
  assert.strictEqual(c.engineVersion, "9.9.9");
  const status = await c.engineStatus();
  assert.strictEqual(status.ok, true);
  assert.strictEqual(status.action, "engine.status");
  // One connection carries the hello and every call.
  await c.call("tool.list");
  assert.strictEqual(engine.connections, 1);
  assert.deepStrictEqual(
    engine.requests.map((request) => request.url),
    ["/local/hello", "/api/v1/actions/engine.status", "/api/v1/actions/tool.list"],
  );
  assert.strictEqual(engine.requests[0].headers["a2m-client-label"], "sdk:tests");
});

test("the channel command's refusal is the client's, and nothing is sent", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  await assert.rejects(
    program.openThroughProgram(process.execPath, [fakeProgram(), "refuse", engine.address]),
    (error) => error.code === "peer_not_allowed" && error.reason === "Not yours." && error.fix === "Stop it.",
  );
  // A command that ends without saying it is open has sent nothing either.
  await assert.rejects(
    program.openThroughProgram(process.execPath, [fakeProgram(), "silent", engine.address]),
    (error) => error.code === "connection_lost" && error.retrySafe === true,
  );
  // A program that is not there is told as the missing program.
  await assert.rejects(
    program.openThroughProgram(path.join(scratch(), "no-such-program"), ["channel"]),
    (error) => error.code === "engine_unreachable",
  );
  assert.strictEqual(engine.requests.length, 0, "nothing was sent");
});

test("a call whose answer is lost through the channel command is not safe to make again", async (t) => {
  const engine = await fakeEngine();
  t.after(() => engine.stop());
  engine.drops.add("tool.call");
  const stream = await program.openThroughProgram(process.execPath, [fakeProgram(), "open", engine.address]);
  const c = await client.greet(stream);
  t.after(() => c.close());
  await assert.rejects(c.toolCall({ tool: "example_orders_order_get" }), (error) => error.code === "answer_lost" && error.retrySafe === false);
});

test("on Windows connect goes through the program's channel command and no pipe is opened by Node", async () => {
  // The program named is not there: the client says so, and opens nothing itself.
  await assert.rejects(
    client.connect({ platform: "win32", start: false, binary: path.join(scratch(), "no-such-program.exe") }),
    (error) => error.code === "engine_unreachable" && /not found/.test(error.reason),
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

test("an action of the saved document that has no handler has no method and no type", () => {
  // Decision 0437, point 5: an action that answers `action_not_built` is
  // described with `x-sdk: false`, and the client has nothing for it.
  const document = JSON.parse(fs.readFileSync(DOCUMENT, "utf8"));
  const declared = fs.readFileSync(path.join(CLIENT_DIR, "actions.d.ts"), "utf8");
  const written = fs.readFileSync(path.join(CLIENT_DIR, "actions.js"), "utf8");
  const unbuilt = new Set();
  for (const item of Object.values(document.paths)) {
    for (const op of Object.values(item)) {
      if (op["x-action"] && op["x-sdk"] === false) unbuilt.add(op["x-action"]);
    }
  }
  for (const action of unbuilt) {
    const name = generator.methodName(action);
    assert.strictEqual(client.Client.prototype[name], undefined, action);
    assert.ok(!client.ACTIONS.includes(action), action);
    assert.ok(!declared.includes(`  ${name}(`) && !written.includes(`"${action}"`), action);
  }
});

test("an input field named like the per-call key is not generated twice", () => {
  const schema = { type: "object", additionalProperties: false, required: ["idempotency_key"], properties: { idempotency_key: { type: "string" }, name: { type: "string" } } };
  const document = { info: {}, paths: { "/api/v1/actions/a.b": { post: { "x-action": "a.b", "x-sdk": true, summary: "S.", requestBody: { content: { "application/json": { schema } } } } } } };
  const { js, dts } = generator.render(document);
  assert.ok(!dts.includes("idempotency_key"), dts);
  assert.ok(!js.includes("idempotency_key"), js);
  assert.ok(dts.includes("name?: string;"), dts);
  assert.deepStrictEqual(generator.requestSchema(document.paths["/api/v1/actions/a.b"].post), { fields: [["name", { type: "string" }]], required: [] });
});

test("a generator that does not understand the document stops", () => {
  const free = { info: {}, paths: { "/api/v1/actions/a.b": { post: { "x-action": "a.b", "x-sdk": true, summary: "S.", requestBody: { content: { "application/json": { schema: { type: "object", additionalProperties: {}, properties: {} } } } } } } } };
  assert.throws(() => generator.render(free), generator.GenerateError);
  assert.throws(() => generator.render({ info: {}, paths: {} }), generator.GenerateError);
  assert.strictEqual(generator.methodName("tool.call"), "toolCall");
  assert.strictEqual(generator.methodName("run.get"), "runGet");
});

test("an action without an SDK face gets no method, and a route that does not say stops", () => {
  const operation = (action, sdk) => ({ "x-action": action, "x-sdk": sdk, summary: "S.", requestBody: { content: { "application/json": { schema: { type: "object", additionalProperties: false, properties: {} } } } } });
  const document = { info: {}, paths: { "/api/v1/actions/a.b": { post: operation("a.b", true) }, "/api/v1/actions/a.skip": { post: operation("a.skip", false) } } };
  const { js, dts } = generator.render(document);
  assert.ok(js.includes("  aB(input"), js);
  assert.ok(!js.includes("aSkip") && !js.includes("a.skip"), js);
  assert.ok(!dts.includes("aSkip") && !dts.includes("ASkipInput"), dts);
  for (const sdk of [undefined, "true", 1]) {
    const unsaid = { info: {}, paths: { "/api/v1/actions/a.b": { post: operation("a.b", sdk) } } };
    assert.throws(() => generator.render(unsaid), generator.GenerateError, String(sdk));
  }
});

test("an underscore in a noun or a verb starts a word, as the dot does (decision 0405)", () => {
  assert.strictEqual(generator.methodName("policy.tools_allowed"), "policyToolsAllowed");
  assert.strictEqual(generator.methodName("spec.import"), "specImport");
  for (const action of ["tool", "a.b.c", "Tool.call", "tool.Call", "a._b", "a.b_", "a.b__c", "a-b.c", "1a.b", ""]) {
    assert.throws(() => generator.methodName(action), generator.GenerateError, action);
  }
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

test("a client that finds no engine has the program start one and is answered", { skip: !BINARY || WINDOWS }, async (t) => {
  const home = scratch();
  // The passphrase is given as a person's shell gives it: a machine with no
  // key store (a Linux runner) has the vault take its root key from it.
  const env = { A2M_HOME: home, A2M_VAULT_PASSPHRASE: "secret-for-tests-only" };
  t.after(() => {
    spawnSync(BINARY, ["engine", "stop"], { env: { ...process.env, ...env }, stdio: "ignore", timeout: 60000 });
    fs.rmSync(home, { recursive: true, force: true });
  });
  // Asked to start nothing, it starts nothing.
  await assert.rejects(client.connect({ env, start: false }), (error) => error.code === "engine_unreachable");
  assert.ok(!fs.existsSync(path.join(home, "run", "engine.sock")));
  // Otherwise the program starts one, as a command would, and the client is
  // answered by it.
  const c = await client.connect({ env, name: "tests", binary: BINARY });
  const status = await c.engineStatus();
  assert.strictEqual(status.action, "engine.status");
  assert.strictEqual(status.exitClass, "succeeded", JSON.stringify(status.body));
  c.close();
  // The engine runs on: a client that starts nothing reaches it.
  const again = await client.connect({ env, start: false });
  assert.strictEqual((await again.engineStatus()).ok, true);
  again.close();
  const log = fs.readFileSync(path.join(home, "data", "logs", "engine-output.log"), "utf8");
  assert.strictEqual(log.split("a client started the engine on demand").length - 1, 1, log);
});

// The path the client takes on Windows, run on every system: `platform` set
// to Windows sends the client through the program's `anything2mcp channel`
// command, which checks the engine's owner as every command does and then
// carries the client's bytes. On Windows CI runs this test alone and fails
// when it is skipped (the bindings job).
test("the client reaches a real engine through the program's channel command", { skip: !BINARY }, async (t) => {
  const home = scratch();
  const env = { A2M_HOME: home, A2M_VAULT_PASSPHRASE: "secret-for-tests-only" };
  const engine = spawn(BINARY, ["serve"], { env: { ...process.env, ...env }, stdio: ["ignore", "ignore", "pipe"] });
  engine.stderr.resume();
  t.after(() => {
    engine.kill();
    fs.rmSync(home, { recursive: true, force: true });
  });
  // Waits on the named condition, an engine that answers, up to a deadline.
  const deadline = Date.now() + 60000;
  let c = null;
  while (c === null) {
    try {
      c = await client.connect({ env, platform: "win32", binary: BINARY, start: false, name: "tests" });
    } catch (error) {
      assert.strictEqual(error.code, "engine_unreachable", String(error));
      assert.ok(engine.exitCode === null, "the engine stopped");
      assert.ok(Date.now() < deadline, "the engine did not answer within a minute");
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
  t.after(() => c.close());
  assert.match(c.engineVersion, /^\d+\.\d+\.\d+/);
  const status = await c.engineStatus();
  assert.strictEqual(status.ok, true, JSON.stringify(status.body));
  assert.strictEqual(status.exitClass, "succeeded");
  // A second call on the same connection: the hello holds for it.
  const unknown = await c.call("no.such");
  assert.strictEqual(unknown.ok, false);
});
