// Types of the Anything2MCP client for Node. The methods are in actions.d.ts,
// which is generated from the engine's OpenAPI document.

import { Actions } from "./actions";

export { ACTIONS, Actions } from "./actions";
export * from "./actions";

/** The local API version this client speaks; the engine refuses another. */
export declare const LOCAL_API: number;

/** The engine was not reached, or its answer could not be read. `code` is the engine's failure code. */
export declare class ChannelError extends Error {
  readonly code: string;
  readonly reason: string;
  readonly fix: string;
  /**
   * True when nothing was sent, so the call may be made again. False for
   * `answer_lost`: the engine may have had the call, which may have started
   * a run that writes.
   */
  readonly retrySafe: boolean;
  constructor(code: string, message: string, fix: string, retrySafe?: boolean);
}

/** A failure as the engine reports it: one class and one code. */
export interface EngineError {
  failureClass: string;
  code: string;
  message: string;
  fix: string;
  retrySafe: boolean;
}

/** `succeeded`, `failed`, `malformed`, or `person_needed`: the engine's exit class. */
export type ExitClass = "succeeded" | "failed" | "malformed" | "person_needed";

/** One action's answer, as the engine wrote it. */
export interface Response {
  ok: boolean;
  action: string;
  exitClass: ExitClass;
  result: unknown;
  error: EngineError | null;
  warnings: unknown[];
  next: unknown;
  /** The whole body, as the engine wrote it. */
  body: Record<string, unknown>;
}

export interface CallOptions {
  /** Makes a repeated write the same write. */
  idempotencyKey?: string;
}

export interface ConnectOptions {
  /** The folder the engine reads target files from. */
  project?: string;
  /** How this program names itself in the engine's audit log. */
  name?: string;
  /** Milliseconds a request may take before the connection is dropped; none by default. */
  timeout?: number;
  /** The environment the engine's place is read from; `process.env` by default. */
  env?: Record<string, string | undefined>;
  /** Overrides `process.platform`, for tests. */
  platform?: NodeJS.Platform;
  /**
   * Windows only. Resolves to true only when the process that serves the pipe runs as this
   * user. Node's standard library cannot tell, so without it the client refuses to connect
   * (`client_not_built`).
   */
  verifyPipeServer?: (socket: import("node:net").Socket) => Promise<boolean> | boolean;
  /** Windows only: the text of the user's security identifier, which the pipe's name carries. */
  sid?: string;
  /**
   * When no engine answers, run the program's `anything2mcp engine start`, which starts one as
   * any command does, and connect to it. True by default; false starts nothing and rejects with
   * `engine_unreachable`.
   */
  start?: boolean;
  /** The program that starts the engine; the one the platform package installed by default. */
  binary?: string;
}

/** A connection to the local engine. Close it when done. */
export declare class Client extends Actions {
  /** The engine's version, from its answer to the hello. */
  engineVersion: string;
  /** Sends one action by its name, such as `engine.status`, with its input. */
  call(action: string, input?: Record<string, unknown>, options?: CallOptions): Promise<Response>;
  close(): void;
}

/**
 * Connects to the local engine that runs as this user. When none answers, the installed program
 * starts one, unless `start` is false.
 */
export declare function connect(options?: ConnectOptions): Promise<Client>;

/** Connects to a socket file or pipe name, runs `verify` on it if given, and says hello. */
export declare function connectTo(
  address: string,
  options?: {
    verify?: (socket: import("node:net").Socket) => Promise<boolean> | boolean;
    name?: string;
    project?: string;
    timeout?: number;
  },
): Promise<Client>;

/** The version of this package. */
export declare function packageVersion(): string;
/** Where the engine's socket is on macOS and Linux. */
export declare function socketPath(env?: Record<string, string | undefined>, platform?: NodeJS.Platform): string;
/** The engine's runtime directory on Windows, as the pipe's tag is made from it. */
export declare function windowsRuntimeDir(env?: Record<string, string | undefined>): string;
/** The name of the engine's pipe on Windows. */
export declare function pipeName(runtimeDirText: string, sid: string): string;
