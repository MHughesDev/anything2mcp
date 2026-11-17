#!/usr/bin/env node
"use strict";

// Writes the Node client's typed methods from the engine's OpenAPI document.
//
// The engine serves the document at `GET /api/v1/openapi.json`; it is made
// from the action registry, so a method written from it cannot name an
// action the engine lacks. `bindings/python/tools/openapi.json` is the saved
// copy, shared with the Python client (`tools/fetch_openapi.py` saves it).
// This script reads it and writes `anything2mcp/client/actions.js` and
// `anything2mcp/client/actions.d.ts`: one method per action, named as the
// action names it in camel case (`tool.call` is `toolCall`), taking one
// object of inputs, with the input types from the request schema and a
// comment from the operation's summary. A method names its action and
// passes its input; it decides nothing.
//
// No dependency, and no compile step: plain `.js` beside plain `.d.ts`.
//
//   node scripts/generate-actions.js           # write both files
//   node scripts/generate-actions.js --check   # fail if either differs

const fs = require("node:fs");
const path = require("node:path");

const HERE = __dirname;
const DEFAULT_DOCUMENT = path.join(HERE, "..", "..", "python", "tools", "openapi.json");
const DEFAULT_DIR = path.join(HERE, "..", "anything2mcp", "client");
const ACTION_ROUTE = "/api/v1/actions/";
// Names the client class already uses.
const RESERVED = new Set(["call", "close", "constructor"]);

class GenerateError extends Error {}

function methodName(action) {
  const parts = action.split(".");
  const name = parts[0] + parts.slice(1).map((part) => part[0].toUpperCase() + part.slice(1)).join("");
  if (!/^[a-z][A-Za-z0-9]*$/.test(name) || RESERVED.has(name)) {
    throw new GenerateError(`action \`${action}\` has no valid method name`);
  }
  return name;
}

function typeName(action) {
  const name = methodName(action);
  return name[0].toUpperCase() + name.slice(1) + "Input";
}

function tsType(schema) {
  if (schema === null || typeof schema !== "object") return "unknown";
  if (schema.enum !== undefined) {
    const values = schema.enum;
    if (!Array.isArray(values) || values.length === 0 || !values.every((v) => ["string", "number", "boolean"].includes(typeof v))) {
      throw new GenerateError(`unsupported enum ${JSON.stringify(values)}`);
    }
    return values.map((v) => JSON.stringify(v)).join(" | ");
  }
  const kind = schema.type;
  if (Array.isArray(kind)) {
    return kind.map((k) => (k === "null" ? "null" : tsType({ ...schema, type: k }))).join(" | ");
  }
  switch (kind) {
    case "string":
      return "string";
    case "integer":
    case "number":
      return "number";
    case "boolean":
      return "boolean";
    case "array": {
      const item = tsType(schema.items);
      return item.includes(" | ") ? `Array<${item}>` : `${item}[]`;
    }
    case "object":
      return "Record<string, unknown>";
    default:
      return "unknown";
  }
}

function requestSchema(operation) {
  const body = operation.requestBody;
  if (!body) return { fields: [], required: [] };
  const schema = body.content["application/json"].schema;
  if (schema.type !== "object") throw new GenerateError("a request body that is not an object");
  if (schema.additionalProperties !== undefined && schema.additionalProperties !== false) {
    throw new GenerateError("a request body that takes fields it does not name");
  }
  const properties = schema.properties || {};
  const fields = Object.keys(properties).sort().map((key) => [key, properties[key]]);
  return { fields, required: schema.required || [] };
}

function actionsOf(document) {
  const found = new Map();
  for (const [route, item] of Object.entries(document.paths)) {
    if (!route.startsWith(ACTION_ROUTE) || !item.post) continue;
    const action = item.post["x-action"];
    if (action !== route.slice(ACTION_ROUTE.length)) {
      throw new GenerateError(`route ${route} names action ${JSON.stringify(action)}`);
    }
    found.set(action, item.post);
  }
  if (found.size === 0) throw new GenerateError("the document has no action routes");
  const names = new Set([...found.keys()].map(methodName));
  if (names.size !== found.size) throw new GenerateError("two actions have the same method name");
  return [...found.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
}

function oneLine(text) {
  return String(text ?? "").split(/\s+/).filter(Boolean).join(" ");
}

function comment(text, indent) {
  const safe = text.replace(/\*\//g, "*\\/");
  const words = safe.split(" ");
  const lines = [];
  let line = "";
  for (const word of words) {
    if (line && indent.length + 3 + line.length + 1 + word.length > 88) {
      lines.push(line);
      line = word;
    } else {
      line = line ? `${line} ${word}` : word;
    }
  }
  lines.push(line);
  return `${indent}/**\n${lines.map((l) => `${indent} * ${l}`).join("\n")}\n${indent} */`;
}

function propertyKey(key) {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key) ? key : JSON.stringify(key);
}

function summaryOf(action, operation) {
  const summary = oneLine(operation.summary);
  if (!summary) throw new GenerateError(`\`${action}\` has no summary`);
  return summary;
}

function render(document) {
  const actions = actionsOf(document);
  const version = (document.info && document.info.version) || "unknown";
  const banner =
    `// GENERATED: do not edit. Written by scripts/generate-actions.js from the OpenAPI\n` +
    `// document the engine serves at /api/v1/openapi.json (engine version ${version}).\n` +
    `// A test fails when this file differs from what the generator writes. To change a\n` +
    `// method, change the action in the engine, save the new document, and run the\n` +
    `// generator.\n`;

  const js = [banner, '"use strict";\n'];
  js.push("// Every action this module has a method for, in the order of the document.");
  js.push(`const ACTIONS = Object.freeze([\n${actions.map(([a]) => `  ${JSON.stringify(a)},`).join("\n")}\n]);\n`);
  js.push("// Each method names its action and passes its input to `call`, which the client");
  js.push("// supplies. It resolves to the engine's response and branches on nothing.");
  js.push("class Actions {");
  const methods = [];
  const dts = [banner, 'import type { CallOptions, Response } from "./index";\n'];
  const dtsMethods = [];
  for (const [action, operation] of actions) {
    const { fields, required } = requestSchema(operation);
    const unknown = required.filter((r) => !fields.some(([k]) => k === r));
    if (unknown.length) throw new GenerateError(`\`${action}\` requires fields it does not define: ${unknown}`);
    const summary = summaryOf(action, operation);
    const name = methodName(action);
    const hasRequired = required.length > 0;
    methods.push(
      `${comment(`${summary} The engine action is \`${action}\`.`, "  ")}\n` +
        `  ${name}(input${hasRequired ? "" : " = {}"}, options = {}) {\n` +
        `    return this.call(${JSON.stringify(action)}, input, options);\n  }`,
    );
    const lines = [comment(`The input of \`${action}\`.`, "")];
    if (fields.length === 0) {
      lines.push(`export type ${typeName(action)} = Record<string, never>;\n`);
    } else {
      lines.push(`export interface ${typeName(action)} {`);
      const ordered = [...fields.filter(([k]) => required.includes(k)), ...fields.filter(([k]) => !required.includes(k))];
      for (const [key, schema] of ordered) {
        const described = schema && typeof schema === "object" ? oneLine(schema.description) : "";
        if (described) lines.push(comment(described, "  "));
        lines.push(`  ${propertyKey(key)}${required.includes(key) ? "" : "?"}: ${tsType(schema)};`);
      }
      lines.push("}\n");
    }
    dts.push(lines.join("\n"));
    dtsMethods.push(
      `${comment(`${summary} The engine action is \`${action}\`.`, "  ")}\n` +
        `  ${name}(input${hasRequired ? "" : "?"}: ${typeName(action)}, options?: CallOptions): Promise<Response>;`,
    );
  }
  js.push(methods.join("\n\n"));
  js.push("}\n");
  js.push("module.exports = { ACTIONS, Actions };\n");
  dts.push("/** Every action this module has a method for, in the order of the document. */");
  dts.push("export declare const ACTIONS: readonly string[];\n");
  dts.push("/** The typed methods. `Client` extends this class. */");
  dts.push(
    `export declare abstract class Actions {\n  /** Sends one action by its name, such as \`engine.status\`, with its input. */\n  abstract call(action: string, input?: Record<string, unknown>, options?: CallOptions): Promise<Response>;\n\n${dtsMethods.join("\n\n")}\n}\n`,
  );
  return { js: js.join("\n"), dts: dts.join("\n") };
}

function main(argv) {
  const check = argv.includes("--check");
  const valueOf = (flag, fallback) => {
    const at = argv.indexOf(flag);
    return at >= 0 ? path.resolve(argv[at + 1]) : fallback;
  };
  const document = JSON.parse(fs.readFileSync(valueOf("--document", DEFAULT_DOCUMENT), "utf8"));
  const dir = valueOf("--out", DEFAULT_DIR);
  const written = render(document);
  const targets = [
    [path.join(dir, "actions.js"), written.js],
    [path.join(dir, "actions.d.ts"), written.dts],
  ];
  if (check) {
    let bad = false;
    for (const [file, text] of targets) {
      if (!fs.existsSync(file) || fs.readFileSync(file, "utf8") !== text) {
        process.stderr.write(`${file} differs from what the generator writes; run it.\n`);
        bad = true;
      }
    }
    return bad ? 1 : 0;
  }
  for (const [file, text] of targets) fs.writeFileSync(file, text);
  return 0;
}

if (require.main === module) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    if (!(error instanceof GenerateError)) throw error;
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = { GenerateError, actionsOf, methodName, render, requestSchema, tsType };
