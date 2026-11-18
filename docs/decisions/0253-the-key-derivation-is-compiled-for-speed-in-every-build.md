# 0253. The key derivation is compiled for speed in every build

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

## Context

A quick measurement found that a command which must start the engine first takes a little over two seconds, where a command to a running engine takes about twenty milliseconds. An investigation with a trace of every phase found where the time goes: about nine tenths of it is one call, the derivation of the vault's root key from the person's passphrase, on a machine whose build has no operating-system key store ([decision 0220](0220-a-first-run-on-linux-needs-no-administrators-file.md)).

That derivation is slow on purpose. Its cost is what protects a passphrase when someone has a copy of the store ([decision 0074](0074-the-vaults-cipher-key-stores-and-other-outside-crates.md)), and its parameters are not changed here. But the two seconds were not the cost that was chosen: they were the chosen cost multiplied by a build without optimization. Measured with the same library and the same parameters, one derivation takes about 1.9 seconds in an unoptimized build, about 0.19 seconds under the workspace's release profile (which optimizes for size), and about 0.13 seconds optimized for speed. Every test that starts an engine pays the unoptimized figure, about seventeen times over in one test file alone.

## Decision

The two crates that carry the derivation, `argon2` and `blake2`, are compiled at the highest optimization level in every profile: in development and test builds, and in release builds, where the rest of the program stays optimized for size. The derived key is the same in every build. The cost parameters are untouched.

## Consequences

- A development or test build starts an engine in a few tenths of a second instead of two seconds, and the test suite loses about half a minute.
- A released program derives the key in about 0.13 seconds instead of about 0.19. The two crates grow slightly; the size of the whole program is to be measured at the next release build and compared with [decision 0191](0191-the-release-profile-builds-one-code-unit-with-whole-program-optimisation-for-size.md)'s figure.
- The budget in [section 225](../spec/2-kernel/225-performance.md) for "process start to ready" cannot hold on the passphrase route, whatever the build: the derivation alone is longer. The section is to state the derivation as a cost of its own, paid once for each start, with a separate figure for the rest of a start. That revision, with figures from a release build of the product itself, is owed and is not made by this record.
- Other findings of the same investigation are separate work: the idle time settable in seconds, so that a test does not wait a real minute; a record of what asked an engine to stop; `engine status` and `engine stop` answering without opening the whole kernel when no engine runs; signals handled before the kernel opens.

## Alternatives weighed

- **Lower the derivation's cost.** It would make a start faster and a copied store easier to attack. The cost was chosen for the second reason.
- **Optimize every dependency in development builds.** It would slow every build for one hot function. Two crates are enough.
- **Leave it, as only development builds are slow.** The release figure also improves by a third, and the test suite's time is paid by every agent and every CI run.
