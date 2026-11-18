# 0261. A proxy value's credentials are removed before any other cut, and the network readers are bounded and read only the kernel

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the crate `a2m-netstate` and [its design](../architecture/crates/a2m-netstate.md). Corrects [decision 0243](0243-the-machine-reads-its-own-network-state-and-sends-nothing.md) in the points named below; the rest of 0243 stands.

## Context

A second reader reviewed the network readers by reading alone. The review found one critical fault and several minor ones. Each was checked against the code before it was changed.

The critical fault held. The proxy cleaner cut a value at the first `?` or `#`, then at the first `/`, and only then looked for an `@`. A password that holds one of those characters was cut in the middle: the part before the cut holds no `@`, so the user name and the start of the password were kept as the host, with `credentials_removed` false. A proxy list was split on `;` and space before each entry was cleaned, so a password that holds either was split the same way. Run against the old code, the new tests showed it: `http://user:pa?ss-word-1@proxy.example:3128/x?q#f` came out as `http://user:pa`, and the list `http=user:pa;ss@h:1;https=h:2` gave an entry `user:pa`. No action serves the reader yet, so nothing has reached a caller. It had to be fixed before any action serves it, because the result would break rule 4 and point 4 of decision 0243.

## Decision

1. **Credentials are removed before any other cut.** In a proxy value, everything up to the last `@` after the scheme is removed first, whatever the password holds. Only then are the query and the fragment cut off. A value whose path holds an `@` loses everything before that `@`, its host included. That is the safe direction, and `credentials_removed` says so.
2. **Only a proxy scheme is kept as a scheme.** The scheme is kept when it is `http`, `https`, `socks`, `socks4`, `socks4a`, `socks5`, or `socks5h`, in any case. Any other text before `://` is read as part of the value. In a value with no scheme, that text may be a user name, and a user name must not be kept by mistaking it for a scheme. One case remains: a value with no scheme whose user name is itself one of those words and whose password starts with `//`. There the user name is shown as the scheme. No byte of the password is shown.
3. **A list that holds an `@` is left out whole.** The Windows proxy list, the Windows bypass list, and a `no_proxy` or `NO_PROXY` value are split on characters a password may hold. Where a password in one of them ends cannot be told. So a list that holds an `@` anywhere gives no entry, and the reading carries an absent entry for it (`list`, `bypass`, or the variable's name). Its reason is a new one, `may_hold_credentials` (rule 7: what cannot be split safely is left out by name, never guessed). `text::proxy_list` now returns the entries and that absent entry, and `text::proxy_from_environment` returns the settings and the absent entries. `text::proxy_bypass` is new.
4. **The reason `not_read` covers a bound.** Its meaning is widened to "this reader did not read it: it is not built to yet, or a bound stopped the search for it".
5. **The default routes are bounded.** They are bounded by `MOST_ROUTES`, like the routes, and a cut is named `default_routes`.
6. **The owner walk is bounded and honest about where it stopped** (Linux). It reads at most `MOST_PROCESSES` processes and at most `MOST_DESCRIPTORS` open descriptors in all (2^20, a new public constant), so its time is bounded. An owner the walk did not reach because a bound stopped it is absent as `not_read`. An owner it could not see is still `not_visible_to_this_user`. The program is now named by the file name of the process's `exe` link, not by its `comm`: a process may set its `comm` to anything, and the record said "the program's file name". The review also said a table's cut replaced an earlier one. That does not hold as a loss: the replacing entry counted the sockets of every table read so far. The code now pushes one entry after the loop, which makes the same count plain.
7. **Only the kernel's answer is read, and a receive waits a bounded time** (Linux). The netlink answer is read with `recvfrom`. A datagram from any sender but the kernel (port 0) is dropped unread, and more than 1024 such datagrams in one dump stop the reader with `net_answer_unreadable`. Each receive waits at most five seconds (`SO_RCVTIMEO`). After that the read is `net_read_failed`, and the blocking thread is never held for good.
8. **The source scan forbids more and scans more.** It also refuses `socket(`, `write(`, `ioctl`, `WSAIoctl`, `Command::new`, and `std::process`. It leaves out only a module written as `#[cfg(test)]` and then `mod tests {`, up to its closing line. Before, it cut each file at the first `#[cfg(test)]`, so code after an early test-only item was never scanned.
9. **What the namespace run proves.** Decision 0243 point 2 counted the namespace run as one of three holds that nothing is sent. The run proves that the readers read their own namespace. It does not prove by itself that nothing is sent: the namespace's one interface is down and has no address, so there is nowhere to send to. That nothing is sent is held by the source scan and by the closed list of dependencies. The test's comment and the crate design now say so.

## Tests

| Test | Failed before |
| --- | --- |
| `text::tests::no_byte_of_a_proxy_password_survives_cleaning_whatever_it_holds` | Yes, for a password holding `/`, `?`, or `#`, with and without a scheme (run against the old body kept under a test-only name, then removed) |
| `text::tests::a_list_that_holds_credentials_is_left_out_whole` | Yes, for both lists |
| `text::tests::a_value_with_no_credentials_is_kept_as_it_was` | No: it guards against a regression |
| `lib.rs`, `the_default_routes_are_bounded_and_their_cut_is_named` | Not run before (the function it calls is new); the old code did not bound the list |
| `linux.rs`, `a_walk_a_bound_stopped_says_the_owner_was_not_read` | Not run before (the walk took no bounds) |
| `netlink.rs`, `a_message_from_another_process_is_dropped_unread` | Yes, with the sender check switched off: a forged end of the dump, sent first by another socket, ended the dump empty |
| `netlink.rs`, `a_kernel_that_never_answers_is_a_read_failure_not_a_wait_for_good` | Not run before: without the time limit the receive would block for good |
| `tests/sends_nothing.rs`, `the_scan_leaves_out_only_the_unit_test_module` | New: it holds the scan's own cut |

Each assertion about a proxy value is made on the value and on its `Debug` form.

## What only CI can prove

The Windows proxy reader's change (the list and the bypass left out when they hold an `@`) was compiled and linted for `x86_64-pc-windows-msvc`, and the Linux readers' changes ran here. Neither the Windows change nor the macOS code has run. The new netlink tests run on Linux only. The namespace run passes on a system that refuses a new namespace unless `A2M_REQUIRE_NAMESPACE=1` is set. Nothing in CI sets that variable yet. Hosted Ubuntu 24.04 runners may refuse unprivileged user namespaces through AppArmor, so setting the variable there may need the runner's restriction lifted first.

## Not done

- Finding A5: on Windows, the UDP table also lists connected endpoints, so "a bound socket with no peer" is true on Linux only. On macOS, an Ethernet-framed interface has no absent entry for its kind.
- Finding A7: more forms in the corpus of address spellings. That corpus belongs to `a2m-core`.

## Alternatives weighed

- **Report each entry's host only when a list holds an `@`.** This keeps more. Refused: where an entry ends cannot be told in such a list, so a host cut from it could itself be part of a password.
- **The reason `not_read` for a list left out.** This is what the build plan proposed. Refused: the list was read. `may_hold_credentials` tells the person why it was left out, at the cost of one more word in the serialized list of reasons.
- **Keep any scheme that is well formed.** Refused: in a value with no scheme, almost every user name is a well-formed scheme, and it would be kept.
- **Keep `comm` and change the record's wording.** Refused: `comm` is whatever the process says, and reading the `exe` link needs the same permission as the descriptor folder that found the owner.
