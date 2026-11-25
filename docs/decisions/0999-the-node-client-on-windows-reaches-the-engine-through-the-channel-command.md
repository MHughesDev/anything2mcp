# 0999. The Node client on Windows reaches the engine through the channel command

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Settles:** the Node half of roadmap package 5.4 on Windows, which [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md) moved after the first release and [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md) brought forward, now package 5.4b. Supersedes the part of [decision 0156](0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md) that has the Node client refuse on Windows unless the caller passes `verifyPipeServer` and `sid`. Changes the public interface of `a2m-local`: it adds `Connector::open_checked` and `CheckedStream`.

## Context

On Windows the engine's local channel is a named pipe. Every client of it must check, before it sends a byte, that the process serving the pipe runs as the client's own user. Without that check, a program of another user that made the pipe first could answer in the engine's place. The engine's own client makes the check, and so does the Python client (decision 0156). It reads the server's process with `GetNamedPipeServerProcessId` on the client's own handle, reads that process's token, and refuses with `peer_not_allowed` unless the token names exactly the client's user. Node's built-in modules cannot do that. They have no foreign-function interface: `require("node:ffi")` is an unknown module in Node 22. A `net.Socket` on Windows does not expose its handle. `fs` cannot read who owns a pipe. So the Node client refused (`client_not_built`) unless its caller supplied a check.

## Decision

**The Node client on Windows runs the program's `anything2mcp channel` command and speaks to the engine through it.** The command is a process command, declared as `stdio` is: in `PROCESS_COMMANDS`, with hand-written help, in the action catalog's table of process commands, and in no other registry.
- **What it does.** It opens the channel with `Connector::open_checked`, which is the same connect and the same check of the server's user that every command makes. It then copies bytes between its standard input and output and the connection, and reads none of them. It sends no hello and makes no request: the client's own hello and requests are what pass through it.
- **What it says.** It writes one line on standard error: `anything2mcp: the local channel is open.` once the checked connection is made, or else the refusal as one JSON object (the `Response` shape a command writes), after which it ends with that refusal's exit code.
- **What it does not do.** It starts no engine. It takes no argument. It is not an authoring or admin action, so it has no MCP tool, REST route, or SDK method. It holds no engine logic: it is a thin client as the stdio proxy is, and less, because it does not even read the protocol.
- **When it ends.** It ends when its standard input ends, when the engine closes the connection, or when its standard output cannot be written.

**The client side.** The client keeps the one connection it says hello on, as it does over a socket. It treats the command's standard input and output as that connection, and it uses one command process per connection, not one per call. `connect` on Windows finds the program as `engine start` does: `binary`, or the one the platform package installed. It waits for the open line, and on a refusal rejects with the command's own code. `engine_unreachable` still starts an engine as before. The client no longer makes the pipe's name, so `pipeName`, `windowsRuntimeDir`, `verifyPipeServer`, and `sid` are removed from the Node package. The Python client is unchanged.

**`a2m-local` exposes the check without the hello.** `Connector::open_checked(&self) -> Result<CheckedStream, Failure>` is the half of `Connector::connect` before the hello. `CheckedStream` is the checked connection as bytes, `AsyncRead` and `AsyncWrite`: a Unix socket on macOS and Linux, the pipe on Windows. `connect` is now `open_checked` followed by the hello, so there is one connect and one rule.

**One code path on every system.** The command is built and works on macOS and Linux too, over the socket with the socket's check. That lets its tests, and the Node client's real-engine test through it, run on every runner. The Node client uses it only on Windows; on macOS and Linux it opens the socket itself, as before.

*Instead of:*
- **A helper process that checks the pipe for the client** (PowerShell, `whoami`, or the program asked a question). It opens its own instance of the pipe, not the client's. So it checks a different connection, and the pipe could change hands between that check and the client's connection. Reading the server's user from PowerShell also needs code compiled at run time. Decision 0156 rejected this too.
- **A proof of the server through the runtime folder** (option B). The client would write a nonce into a file in the folder that only the user can open, and the engine would show that it read it. That needs a new step in the hello, and it is a different rule: it proves the server can read as the user, not that the server's token is the user's. Rule 2 and "the same rule, not a weaker one" weigh against it.
- **A native add-on.** That is another language, a build per platform, and a dependency (rule 6).
- **Connecting with no check.** Safe comes first.

## Consequences

- **Tests that fail without the change:**
  - `a_checked_stream_carries_the_callers_own_hello_and_is_refused_by_the_same_rule` in `a2m-local`: no engine is told by name; an engine of another user is refused with `peer_not_allowed` before anything reaches it; a caller's own hello and request reach the service.
  - `the_channel_command_carries_the_clients_own_hello_and_calls_to_the_engine` and `with_no_engine_the_channel_command_says_so_and_starts_none` in the product's tests.
  - The Node client's tests against a stand-in for the command, run by Node on every system: hello and calls, a refusal, a command that ends silently, a missing program, and a lost answer.
  - `the client reaches a real engine through the program's channel command`, run on every runner with the built program.
- **The CI step.** "Prove the Node client calls the real engine by its pipe on Windows", in the `bindings` job, runs that last test alone on Windows and fails if it was skipped. Until it has run there, the Windows half is written and not proven.
- A request on the Node client's connection on Windows passes through one more process. Nothing was measured.
- On Windows the engine's audit names the client by the label of the Node client's own hello. The process it sees is the command's.
