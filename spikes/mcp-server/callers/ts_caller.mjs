#!/usr/bin/env node
// Caller built on the reference client library for TypeScript, from npm.
//
// Usage: ts_caller.mjs <v1|v2> <mode> <clients-folder> <report.json> <server command...>
//   v1: the package `@modelcontextprotocol/sdk` (handshake revisions only); mode is ignored.
//   v2: the package `@modelcontextprotocol/client`; mode: legacy | auto | pin.
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const [line, mode, clientsDir, reportPath, ...command] = process.argv.slice(2);
const require = createRequire(path.resolve(clientsDir, 'package.json'));
const load = (name) => import(pathToFileURL(require.resolve(name)).href);
const TOOL = 'example_orders_order_get';
const installed = (name) => JSON.parse(readFileSync(path.resolve(clientsDir, 'node_modules', name, 'package.json'), 'utf8')).version;

let Client, StdioClientTransport, pkg;
if (line === 'v1') {
  ({ Client } = await load('@modelcontextprotocol/sdk/client/index.js'));
  ({ StdioClientTransport } = await load('@modelcontextprotocol/sdk/client/stdio.js'));
  pkg = '@modelcontextprotocol/sdk ' + installed('@modelcontextprotocol/sdk');
} else {
  ({ Client } = await load('@modelcontextprotocol/client'));
  ({ StdioClientTransport } = await load('@modelcontextprotocol/client/stdio'));
  pkg = '@modelcontextprotocol/client ' + installed('@modelcontextprotocol/client');
}

const report = { client: 'reference client library for TypeScript', package: pkg, mode: line === 'v1' ? 'legacy (the only one)' : mode, steps: {} };
const step = async (name, fn) => {
  try {
    report.steps[name] = { ok: true, got: await fn() };
  } catch (e) {
    report.steps[name] = { ok: false, error: { type: e?.constructor?.name, code: e?.code, message: String(e?.message ?? e).slice(0, 300) } };
  }
};
const options = line === 'v1' ? { capabilities: {} }
  : { capabilities: {}, versionNegotiation: { mode: mode === 'pin' ? { pin: '2026-07-28' } : mode } };
const client = new Client({ name: 'spike-ts-caller', version: '0' }, options);
const transport = new StdioClientTransport({ command: command[0], args: command.slice(1), stderr: 'ignore' });
const call = (args, opts) => (line === 'v1' ? client.callTool({ name: TOOL, arguments: args }, undefined, opts) : client.callTool({ name: TOOL, arguments: args }, opts))
  .then((r) => ({ is_error: r.isError, structured: r.structuredContent, text: (r.content ?? []).map((c) => c.text) }));

try {
  await client.connect(transport);
  report.negotiated = client.getNegotiatedProtocolVersion?.() ?? '(see the wire log)';
  report.era = client.getProtocolEra?.();
  report.server_info = client.getServerVersion();
  report.server_capabilities = client.getServerCapabilities();
  await step('tools_list', async () => {
    const r = await client.listTools();
    const t = r.tools[0];
    return { count: r.tools.length, name: t.name, has_output_schema: t.outputSchema !== undefined, annotations: t.annotations };
  });
  await step('call_ok', () => call({ order_id: 'A-1001' }));
  await step('call_not_found', () => call({ order_id: 'Z-0' }));
  await step('call_bad_arguments', () => call({}));
  await step('call_unknown_tool', async () => {
    const r = line === 'v1' ? await client.callTool({ name: 'example_no_such_tool', arguments: {} }) : await client.callTool({ name: 'example_no_such_tool', arguments: {} });
    return { is_error: r.isError };
  });
  await step('call_with_progress', async () => {
    const seen = [];
    const r = await call({ order_id: 'A-1002', wait_ms: 200 }, { onprogress: (p) => seen.push(p.progress) });
    return { ...r, progress_seen: seen };
  });
  await step('call_then_cancel', async () => {
    const ac = new AbortController();
    setTimeout(() => ac.abort('caller cancelled'), 200);
    let gaveUp = false;
    try {
      await call({ order_id: 'A-1002', wait_ms: 2000 }, { signal: ac.signal });
    } catch {
      gaveUp = true;
    }
    await new Promise((r) => setTimeout(r, 300));
    return { client_gave_up: gaveUp };
  });
  await step('call_after_cancel', () => call({ order_id: 'A-1002' }));
  await step('ping', async () => { await client.ping(); return 'empty result'; });
  await client.close();
} catch (e) {
  report.connect_error = { type: e?.constructor?.name, code: e?.code, message: String(e?.message ?? e).slice(0, 400) };
  try { await client.close(); } catch { /* nothing to close */ }
}
writeFileSync(reportPath, JSON.stringify(report, null, 1));
console.log(JSON.stringify(report));
