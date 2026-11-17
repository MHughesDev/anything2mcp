# 57. Supervising subsystems

The runtime starts, talks to, and stops many things it did not write: browsers, plug-in processes, upstream MCP servers, command-line programs, scripting hosts, device links. One part of the kernel, the supervisor, does this the same way for all of them. Most of the rules here were learned from how REA controls analysis engines ([research note 16](../research/16-rea-methods.md)), which is the part of that project most useful to us.

## What the supervisor owns

| It owns | It never does |
| --- | --- |
| Every process the runtime starts, and every connection to one | Signal, stop, or attach to a process it cannot prove it started |
| The handshake with each subsystem | Trust a subsystem that fails its handshake, partly or at all |
| Time limits, cancellation, and cleanup | Report cleanup as done when it could not be confirmed |
| How many of each may run | Fetch a program. It starts what is installed |

## Starting something

1. **Identify it.** The program is found at a declared place and its digest is taken. A digest that matches no known release is not refused; it is recorded as unverified, and everything that passes through the subsystem carries that mark ([section 59](59-the-evidence-ladder.md)).
2. **Give the launch an identity.** A random run identifier and a random token, new for each launch.
3. **Hand them over privately.** Through a pipe the child inherits, or a file only the user can read that is deleted once read. Never on a command line, where other programs can see it.
4. **Start it in a group the runtime can end as a whole.** A process group on POSIX systems; a job object on Windows, set to end with its owner.
5. **Shake hands before any work.** The subsystem must return the run identifier, its own version, the limits it will honor, and, where it is attached to something, the identity of that thing. A subsystem attached to a different file, page, or document than expected is refused.

## Talking to it

| Rule | Detail |
| --- | --- |
| Local transports only | Standard input and output, a socket only the user can open, or a named pipe with an access list. A loopback port only where the platform leaves no choice, and then the first frame must authenticate within a short limit |
| One framing, one reader | Messages are framed the same way for every subsystem, with a size ceiling agreed at the handshake. One shared reader handles framing, ceilings, and matching replies to requests. A bridge supplies only its own envelope |
| A broken stream is not repaired | An unknown reply, an oversized frame, or malformed text makes the whole connection untrusted. It is closed and everything pending fails |
| Every request has a deadline | The deadline comes from the step's time limit. Nothing waits without one |
| Every failure says whether the request ran | `not_started`, or `unknown`. For a write, `unknown` is the class `possible_commit`, and reconcile decides. This is the same rule the ledger already applies to a lost connection |
| Receipts where the subsystem can give them | A subsystem that can report "request 41 completed" after the fact lets a lost reply be settled without guessing |

## Subsystems that do one thing at a time

A scripting host, a debugger, a device link, and a desktop window all serve one request at a time.

- Requests queue in order. The queue has a length, and a full queue answers `busy` at once.
- Cancelling a request that is already on the wire does not make the subsystem free. Either the caller is released while the subsystem is still shown as busy, or the connection is closed and the subsystem restarted. The supervisor never pretends the work stopped.
- For anything a person can also touch, such as an open document, each request carries the state it expects, and a request against a changed state is refused before it acts.

## Long-lived connections

Some subsystems are worth keeping open: a browser, a scripting host with a large document loaded, a device link.

| Rule | Detail |
| --- | --- |
| Named and listed | Each has an identifier, and an action lists what is open, since when, and for whom |
| Counted | Each kind has a limit. A stopped one still holds its place until it is released, so a failure cannot hide behind a new start |
| One start at a time per place | Two callers asking for the same subsystem at once get one start |
| A health state, with what to do | `idle`, `busy`, `exited`, or `unreachable`; and one of retry, wait, or restart |
| Recycled between runs | After a number of runs or past a memory limit, as section 52 already says for the browser |

These are not sessions. A session is a signed-in state and belongs to the session keeper. A connection to a subsystem may carry a session; the two end separately.

## Stopping something

- The runtime ends only what proves it belongs to the runtime. At the moment of stopping, each process in the group is checked for the launch's token. A process that cannot be checked is not skipped and not killed: cleanup is reported as incomplete.
- Incomplete cleanup is a state, not a log line. It names what remains, it blocks new starts of that kind until it is retried, and `doctor.run` shows it.
- The process list comes from the operating system through fixed system paths, so that nothing on a caller's search path can hide or replace it.
- On shutdown every caller that asks joins the same cleanup. Anything that failed to stop stays owned and is retried.
- Cancellation, the failure that caused it, and the result of cleanup are three separate facts in the trace. One never overwrites another.

## Bridges inside a host application

Many systems are best reached from inside: an application with a scripting interface, an add-in mechanism, or an automation interface can run a small piece of code that answers typed requests. This is the bridge archetype of [section 54](54-interaction-archetypes.md), and it is how REA places itself inside the analysis tools it drives.

| Way of getting in | When | Rule |
| --- | --- | --- |
| Start the host with the bridge | The host can be started with a script and no window | The supervisor owns the whole process |
| Start the host, then load the bridge | The host has a window and a start-up hook | The supervisor owns the process; the person sees the host appear |
| Attach to a host the person started | The host is already in use | The person installs the bridge once, knowingly. The bridge writes a descriptor only that user can read. Runtime and bridge each prove themselves to the other before any request. The supervisor never stops the host |

For every bridge:

- It does nothing when loaded. It only answers.
- It exposes a closed list of functions, declared in the provider's manifest. It is not a way to run arbitrary code in the host.
- It is versioned with its provider, and its own digest is part of the handshake.
- It states whether it can only read.
- Installing it into a host is an act that needs a person ([section 61](61-effects-hazards-and-consent.md)), and removing it is one action.

## What `doctor` reports

For each provider and each subsystem it needs: `healthy`, `component_missing`, `runtime_failure`, `unsupported_host`, `unsupported_target`, `config_drift`, or `unknown`, each with a fix. Where something cannot be established, it says so; it never rounds up to healthy. The check itself changes nothing and starts nothing it does not stop.

## Open points

- The shared framing: newline-delimited JSON with a byte ceiling is the likely choice, the same as the plug-in protocol.
- Which operating-system mechanisms confine a plug-in process on each platform. Research is needed before promising more than the manifest check.
- Whether leftovers from an engine that crashed can be recognized and cleaned at the next start. REA writes an ownership record for this and its reader was not found; we need the reader.
