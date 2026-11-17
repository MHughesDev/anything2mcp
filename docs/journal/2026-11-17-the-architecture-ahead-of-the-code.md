# 2026-11-17: the architecture, ahead of the code

## State at the end

Every crate now has a design, and one page says which crate defines, implements, and joins every interface between them. Five further packs are designed as spec sections 68 to 72: command, data, file, stream, device. One roadmap lists every work package to version 1.0 with what it needs and what proves it. No product code changed. The owner then asked for a full restructure of the repository around the new product; that is the next entry.

## Done

- **The architecture,** in `docs/architecture/`: the spine table of who defines, implements, calls, and joins each interface; five flows traced through the crates; where every piece of state lives; and a page for the parts that are not crates.
- **A design for each of the 24 crates,** in `docs/architecture/crates/`, written by five agents in parallel from one brief and then reconciled against each other. Each has the same parts: what it owns, its interface as a Rust sketch, who uses it, its state, its failures, its invariants with the test that proves each, and its build order.
- **Five pack designs,** spec sections 68 to 72, by three agents. Each answers every item of the entry bar in section 55 and designs its own test stand.
- **The roadmap,** `docs/plan/roadmap.md`: twelve milestones, every package with its crates, its needs, its proof, and its state. The packages ready now are listed at its end.
- **The audit** now requires a design for every crate, stating the layer the layering table gives, and refuses the engine depending on the compiler or on policy. Its self-test has 26 cases.
- **Package A7:** the spec index marks every section as kernel, a named pack, or product-wide.

## Learned

- **Drawing the interfaces showed the layers were wrong.** The compiler needs expressions; policy and the keeper need the store; the registry needs the interfaces of auth and sources. All four pairs shared a layer. Decision 0061 redraws them in eleven.
- **The engine must not touch the store.** Three agents reached it separately: a runner on another machine has no store, so everything the engine needs has to come through one interface, `Control`. In one process that is a direct call. This is the largest correction to the first spine.
- **Closed lists in the kernel do not survive a second kind of system.** Surface kinds were a closed list of four, and every one of the five new packs needed another. Decision 0062 makes every word that arrives with a pack a registered name.
- **Packs need to share data, and must not share code.** One pack's mode reads another pack's file kind; one pack's findings feed another's drafting rule. The answer is a schema with one owner, handed across as data by the compiler.
- **Approving cannot be an action.** "Every entry is an action" had one exception waiting: if approving were an action it would have a command and a tool. It is a handle given only to the approval surface.
- **Parallel design agents disagree where the brief was silent,** and nowhere else. Where sign-in providers live, where record locks live, where the plan's types live: each was designed two ways. A second round, sending each agent the decisions, cost a tenth of the first and brought the designs into line.
- **"Discovery cannot write" has no single meaning.** For a browser it is intercepted requests; for a program, only its declared help and version forms; for a database, a read-only transaction; for a queue, never consuming. Each pack states its own.

## Changed in the design

- Decisions 0061 to 0064: the layers redrawn; registered names and shared data; the kernel's interfaces as the crate designs settle them; five packs designed ahead.
- Hypotheses H-32 to H-39.
- `site_tools` is a mode of the web pack, not the MCP pack.
- The technical reference's driver sketch, plug-in protocol table, and auth provider table now point to the crate designs, with a table of where each sign-in provider lives.
- The glossary's definition of a mode, and seven new terms.

## Corrections the specification still owes

The crate and pack designs are ahead of these sections. Each is to be corrected when the specification is restructured.

| Section | What to change |
| --- | --- |
| 6 | The default order of modes comes from each mode's rank in its provider's manifest. An application built on a browser runtime is a web surface |
| 26, 58 | Capturing a person's own use (a watch tab, where the person may save) is distinct from exploring (where a write is impossible) |
| 40 | The build toolset's cap of fifteen tools against some twenty-three actions marked for it |
| 42 | The preview for a publish has no current value; the failure class to run state table; what happens to a lock when a run stops |
| 44 | A session with no identity, such as an open link on a fieldbus |
| 45 against decision 0034 | Whether decisions on reads may be sampled |
| 46 | Lowering comes before counting commit points; decimals in expressions |
| 47 | Who masks a screenshot: the pack that took it |
| 50 | How an argument, a column, or a field carries the identity stamp |
| 55 | Where today's files go, with one owner per kind and readers declared; `site_tools` |
| 56 | An upstream MCP server's manifest is its mapping file |
| 57 | A one-shot child that performs no handshake; a link whose protocol someone else fixed |
| 59 against 63 | A device whose firmware version is outside its map stops reads too |
| 61 | A patch save that passes the untouched check carries no hazard; the kernel's file access is the checked path in `a2m-provider` |
| 62 | Another machine is reached through a runner on it, not a remote session |
| 64 against 20 and 22 | Found tools on a revision with no per-connection lists |
| 67 | A file a target names becomes a document draft; a file a pipeline reads becomes a source draft |
| File formats | Surface kinds as registered names; `stands`; the new file kinds; `scale` on field types |
| Ledger schema | Evidence named by a keyed digest; the tables for wrapped keys, saved session state, scout stages, and the session key on jobs |
| Steps reference | The step kinds each pack adds |
| Action catalog | Actions for starting a project, exporting a toolset's description, managing webhooks, and listing open subsystems |

## Noticed, not acted on

- The data pack's natural test stand, the embedded database, is in the public domain, which is not on the closed licence list. It must be decided on purpose.
- Where an authorization-code response arrives on a server, when no loopback address reaches the person's browser.
- Whether `Verb` stays a closed list. The crate design for `a2m-core` gives both sides.
- The proposed failure codes of every design are in its "Failures" table and in none of the references. They are settled as each crate is built.
- The command pack's design is 340 lines and the provider's is 440. Both are longer than asked, and both earn it.

## Decided without the owner having read it

Records 0061 to 0064, and every choice in the crate and pack designs.

## For the owner

Nothing new. The three items of the last entry stand: the PyPI publisher and the release button, a first publish on crates.io and npm, and private vulnerability reporting.

## Next

The restructure the owner asked for: the specification reorganized into parts, with the web-era sections consolidated and the corrections above applied; the old plans archived; then the packages marked ready in the roadmap, starting with B2.
