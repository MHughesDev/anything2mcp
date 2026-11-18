# 0220. A first run on Linux needs no administrator's file

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** [record 0123](0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md), which said that on Linux the protected setting allowing the passphrase route "must be chosen by a person"; and the remedy of the presence prompt for a missing policy file in [record 0142](0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md).

## Context

A person ran the README's commands by hand on Linux, in an empty folder. Four things stood in the way of a first hour:

1. This build has no operating-system key store on Linux (record 0123), so the vault must take the passphrase route. `vault.backend` may be set only in the installation's settings file, `/etc/anything2mcp/settings.toml` unless `A2M_HOME` is set, and with no such file every command after `init` failed with `engine_degraded`. A newcomer had to write to `/etc` as an administrator before the first read.
2. An engine started on demand that could not open its vault stayed up, and every later command talked to it, degraded, until someone learned to stop it.
3. A publish could not be finished on a Linux desktop until an administrator copied a polkit policy file that no package ships, from a place the product did not name.
4. `A2M_HOME` inside the project was refused without saying why.

The rule that decides each point is the second of the ten: safe, then correct, then fast, then easy.

## Decision

**1. Where the build has no operating-system key store, the passphrase route is the default.** The protection on `vault.backend` exists so that a lower layer cannot move a machine off a stronger store onto a weaker one. Where the build has only one route there is nothing stronger to move off. So the default of `vault.backend` is `DEFAULT_BACKEND` of `a2m-vault`: `os_keystore` where `OsKeyStore::IN_THIS_BUILD` (Windows, macOS), and `passphrase` where not (Linux). It is a constant from the same `cfg` that chooses the key store's back end: a fact of the build, never a guess made at run time. The setting stays installation-only: a user or environment layer that sets it is still refused, `setting_protected`. On Windows and macOS nothing changes. `PassphraseRoute::of_backend` reads the route from the setting's value.

**2. The passphrase comes from a person or the person's environment.** `A2M_VAULT_PASSPHRASE` is read by the command that starts the engine and handed to it on a pipe, as before (record 0165). Nothing new lets a tool call, an MCP request, or a project file supply it. When the route is chosen and none came, the failure is `input`/`vault_passphrase_missing`, made by `PassphraseKeyStore::not_given(variable)`: it names the variable and says what the passphrase protects. The passphrase protects what the engine keeps on disk: a copy of the store, its key records, and the salt opens nothing without it. It does not protect against a program of the same user while the engine runs: the engine's process holds the derived root in memory, and the passphrase sits in the environment of the shell it came from. The vault's design says so.

**3. An engine started on demand that cannot open its vault ends, and says why.** Two ways were weighed:

- (a) It exits with the named failure, and the next command starts a fresh engine with whatever that command's environment gives.
- (b) It stays up and takes the passphrase from a later command by the same person, over the local channel, peer-checked.

(a) is chosen. It adds no new way for a secret to arrive at the engine; (b) adds a route for the passphrase through the channel that every client speaks, which a steered agent's client speaks too, and a state, "up and waiting for a key", that every door would have to explain. The engine writes one line to its output as it stops, made by `stopped_line` of `a2m-local`: the line's fixed beginning, its process's number, and the failure's class, code, sentence, and next step as JSON, never what a system said. `Connector::connect_or_start` reads the end of the output file of the engine it started, finds the line with that engine's process number, and answers with that failure and the log's path. A line of another process, one that claims a possible commit, and one too long to be the engine's own are not taken; then the client says, as before, that the engine ended before it answered. An engine that had its vault open and loses it is a different case and is not changed: what it holds in hand stays.

**4. The presence prompt says how to make itself available.** The policy file is in the binary (`include_bytes!`). When polkit does not know the engine's action, the availability writes a copy into the folder `polkit` of the engine's data directory: the folder made 0700 and checked to be the user's, the file written whole under another name, never through a link, moved into place, mode 0644, and written again when it differs. The remedy names the copy and gives one command for a person with administrator rights:

```sh
sudo sh -c 'f=$(mktemp) && cat <copy> > "$f" && echo "<sha-256>  $f" | sha256sum --check --quiet && install -m 0644 "$f" /usr/share/polkit-1/actions/io.github.mhughesdev.anything2mcp.presence.policy; s=$?; rm -f "$f"; exit $s'
```

The engine never runs it, never asks for elevation, and writes nothing outside its data directory. The other reasons were each read again: no system bus and a session that is not local and active now say plainly that nothing can approve on that machine and that this is by design, because a write needs a person at the machine; no polkit says that an administrator installs it; a yes given without asking says to run the engine as one's own account, or what an administrator removes. The approval page and the answer to a press escape the text as any text; `doctor` gets it whole, to escape at its own door.

**5. `A2M_HOME` inside the project.** The refusal is not made in `a2m-local` or `a2m-vault`, and a search of the product's code and a run of the built program with `A2M_HOME` inside a project did not find it. The sentence its message should carry is in the lead's report; nothing is changed here.

## Rejected

- **Leave the route a person's choice on Linux, and make the README write the file.** It keeps a step that protects nothing on a build with one route, and the file must be in `/etc` or under `A2M_HOME`.
- **Choose the default at run time,** by trying the key store first. A machine whose store is briefly unreachable would move to a weaker route by itself, which is the move the protection exists to forbid.
- **The plain command `sudo install -m 0644 <copy> /usr/share/polkit-1/actions/`.** The copy is in the user's folder, and any program of the user, a steered agent with a shell among them, can change it before the administrator runs the command. A policy file can declare any action with any defaults, so the administrator would install what the user's programs chose. The command chosen checks the bytes it installs, after it has copied them where only the administrator can write; a copy changed before or during the command is refused, and nothing is installed. A path with anything but letters, digits, and `/._-+,@` is not put in a command at all.
- **A key file beside the vault, a passphrase in a settings file, a default passphrase, or a development mode.** Each makes the first run easier by making every run weaker.

## What was proven

- On Linux, with an empty folder and no settings file, the default chooses the passphrase route and a vault seals, and opens after a restart (`where_the_build_has_no_key_store_the_passphrase_route_is_the_default_and_opens`).
- An engine started on demand that stops by name is answered by that name, twice in a row with a fresh engine each time, and a line of another process is not taken (`an_engine_started_on_demand_that_stops_by_name_is_answered_by_that_name`).
- The copy is written whole and through no link; the command installs it only when its bytes are the binary's, run into a folder of the test's; the fake authority's "not registered" gives the copy and the command.
- Against a real polkit in a container, all five stages of the CI job's test passed, with the `no_policy` stage asserting the copy and the command. The policy file was then installed by running the remedy's own command as root: a copy changed first was refused and nothing was installed; the true copy was installed, and polkit knew the action within a moment, without a restart.

## Not done here

- The default in the settings schema, and the engine's and the client's use of the new failure and of `stopped_line`, are in crates another agent owned at the time. Until those lines land, Linux still needs the installation file.
- An engine given the wrong passphrase opens, because a passphrase store has nothing to check against until the first key is unwrapped; it fails `vault_passphrase_mismatch` at the first seal or open. The engine could unwrap one key at start to find it.
- **When the Secret Service back end returns** (record 0123), `OsKeyStore::IN_THIS_BUILD` turns true on Linux and the default there becomes the key store again. An installation that sealed under the passphrase must then keep `backend = "passphrase"` in its installation file, or its keys stop opening. That release needs a note and a check in `doctor`.
