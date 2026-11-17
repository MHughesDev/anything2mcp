# Spike: the base of the browser engine

Roadmap package 0.4, with research brief 1. Throwaway code: it answers one question and is never promoted into the product. The answer is in [research note 17](../../docs/research/17-browser-engine-base.md) and [decision 0070](../../docs/decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md).

**The question.** What does the browser engine stand on: a DevTools-protocol client of our own over the debugging pipe, an existing Rust DevTools crate, or a WebDriver client?

## What is here

| Folder | What | Run? |
| --- | --- | --- |
| `common/` | What every program shares: the test application started in the same process, a data directory the program owns under the system's temporary folder, the timers, and the check that no browser process is left | Yes |
| `a-own-pipe/` | Candidate (a). `src/cdp.rs` is the whole protocol client; `src/launch.rs` hands the pipe to the browser, two ways on POSIX and one on Windows; `src/main.rs` is the steps | Yes, on Linux. The Windows part compiles for `x86_64-pc-windows-msvc` and was never run. The POSIX part also compiles for `aarch64-apple-darwin` |
| `b-chromiumoxide/` | Candidate (b): the one existing DevTools crate that passed the licence and maintenance checks and has no default that breaks a product rule | Yes, on Linux |
| `c-thirtyfour/` | Candidate (c): a WebDriver client | Up to the session, which the driver program on this machine refuses because it is for another browser version |
| `checked-not-run/` | Six more crates. Only the dependency tree of each was checked, and its source read. Each failed a check before a program was worth writing; the research note says which | No |
| `z-baseline/` | The shared harness alone, so that a candidate's build time and crate count can be given as a difference | Yes |
| `results/` | What the programs printed: three runs each | |
| `measure.sh` | Prints the crates each candidate adds and what the licence list refuses | Yes |

## The steps every program does

1. Start the test application on a port it chooses.
2. Start the browser with a data directory the program owns.
3. Connect, and open the form for line item `L-101` of order `A-100`.
4. Wait for a named condition: the field labelled "Description" is laid out. No sleep.
5. Read that field's value.
6. Read it 200 more times on the warm page.
7. Take a snapshot of the accessibility tree.
8. Intercept the request the form sends when "Save" is clicked, fail it before it leaves the browser, and ask the test application's control interface how many writes it stored. The answer must be 0.
9. Close the browser and check that no process naming the data directory is left.
10. Start again, leave one call pending, kill the browser, and record how the call ends.

The program for candidate (a) also measures a locate through the accessibility tree against a locate by a helper in the page, an observer against a 50 ms poll on a late element, and a second tab on the warm browser.

## Running it

Each folder is a crate of its own, outside the product's workspace. Give the spike its own build folder.

```text
cd spikes/browser-base/a-own-pipe
CARGO_TARGET_DIR=../target/a-own-pipe cargo run --release            # descriptors mapped by a helper crate
CARGO_TARGET_DIR=../target/a-own-pipe cargo run --release -- shell   # descriptors moved by a shell, no unsafe anywhere
cd ../b-chromiumoxide
CARGO_TARGET_DIR=../target/b-chromiumoxide cargo run --release
cd ../c-thirtyfour
A2M_SPIKE_DRIVER=/path/to/the/driver CARGO_TARGET_DIR=../target/c-thirtyfour cargo run --release
```

The browser program is `/opt/pw-browsers/chromium` unless `A2M_SPIKE_BROWSER` names another. The product finds a browser differently: see the decision.

Every program passes `--no-sandbox`. That is for the container this ran in, which runs as root. The product never passes it by default.

## What the spike does not show

- Nothing ran on Windows or macOS.
- No long session: no program ran for more than a few seconds.
- No memory figure, and no comparison with a Playwright sidecar.
- One page of the test application, one field, one browser build.
