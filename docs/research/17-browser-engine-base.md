# Research: the base of the browser engine

**Status:** completed on 2026-11-17, by a spike and by reading source. Answers research brief 1 of [open research](11-open-research.md) and roadmap package 0.4. Feeds [spec section 302](../spec/3-packs/302-the-web-pack-browser-engine.md), the design of [the browser engine crate](../architecture/crates/a2m-pack-web-browser.md), and [decision 0070](../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md). The spike is in `spikes/browser-base/` and is never promoted.

Everything was run on Linux only, in a container that runs as root, against one browser build: Chrome for Testing 153.0.8010.12, headless, with `--no-sandbox`. That flag is for the container only. Each claim below says whether it was run, read in source, or reasoned.

## Question

What does the browser engine stand on?

- (a) A DevTools-protocol client of our own, over the debugging pipe the design requires ([section 204](../spec/2-kernel/204-supervising-subsystems.md)) in place of a port.
- (b) An existing Rust DevTools crate.
- (c) A WebDriver client.

## Answer

(a). A client of our own is 166 lines, adds seven small crates, and is as fast as the best existing crate, because the browser is the cost. No existing crate that passes the product's rules can use the pipe on all three operating systems. Each one that came close either listens on a loopback port, hides that it is automation by default, sends usage data by default, downloads a browser by itself, or carries a licence the closed list refuses. What an existing crate would save is the part that is small. The hard part, locating by meaning and waiting on events, none of them does the way the design needs, so it is ours to write whichever base is chosen.

## The candidates

Every crate on crates.io that speaks the DevTools protocol or WebDriver and had a release in the last two years was looked up on 2026-11-17. "Licence tree" is the result of the repository's licence check, `cargo-deny` with the closed list in `deny.toml`, on a crate that depends on the candidate. Crate counts are what the candidate adds, on Linux, to the harness every program shares.

| Candidate | Newest release | Licence tree | Pipe | Starts or fetches a browser | Defaults against the product's rules | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| (a) Our own client | | Passes. Adds 7 crates: `command-fds`, `nix`, `thiserror` and its macro crate, `cfg-if`, and two that Tokio's process feature brings | Yes. Run on Linux; compiled for Windows and macOS | Starts the program it is given. Fetches nothing | None | **Chosen** |
| (b) `chromiumoxide` 0.9.1 | 2026-02-25; three releases in twelve months | Passes with default features off. Adds 44 crates | No. Starts the browser with `--remote-debugging-port` and reads the address from the browser's error output; connects by WebSocket only | Starts one it finds by an environment variable, the search path, the Windows registry, or known places. Fetches only with the `fetcher` feature, which is off by default | None by default. A stealth mode exists and is off | Works; refused for the port, the size, and what it does not do |
| (b) `headless_chrome` 1.0.22 | 2026-06-11 | **Fails.** Its build dependency `auto_generate_cdp` 0.4.6 declares its licence by a file, and the file is the GNU General Public License, version 3 | No. A port | Fetches only with the `fetch` feature | Its wait for an element asks again every 100 ms. Threads, not Tokio | Refused: licence |
| (b) `chromey` 2.58.2 | 2026-09-12 | Needs ISC and CDLA-Permissive-2.0 added, and brings a C library to build. Adds 102 crates | No. A port | As `chromiumoxide`, which it was forked from | **Its default arguments include `--disable-blink-features=AutomationControlled`,** which hides the mark of automation. It depends on a fingerprint crate | Refused: rule 5 |
| (b) `rustwright` 0.3.0 | 2026-09-30; alpha; first release 2026-07-20 | Needs ISC and CDLA-Permissive-2.0 added. Adds 46 crates | On POSIX only, and only when an environment variable asks. On Windows the pipe is refused with an error | Looks in another project's browser cache. No fetch found | **Sends a usage event to an analytics host on first launch** unless an environment variable switches it off. **Its default arguments include the same flag as `chromey`** | Refused: nothing phones home, and rule 5 |
| (b) `ferrous-browser` 0.1.2 | 2026-05-26; three releases in three weeks and none since; 115 downloads in all | Passes. Adds 63 crates | No. A port | Starts one | **Always passes `--no-sandbox`** | Refused: not maintained |
| (c) `thirtyfour` 0.38.3 | 2026-11-14 | Passes with default features off. Adds 30 crates | Not applicable: it speaks HTTP to a driver program | Its default `manager` feature fetches driver programs | The wait is a poll | Refused: see "WebDriver" below |
| (c) `fantoccini` 0.22.2 | 2026-11-15 | **Fails.** Depends on `webdriver` 0.54.0, which is MPL-2.0 | Not applicable | No | | Refused: licence |
| (c) `rustenium` 1.1.15 | 2026-10-25 | Passes. Adds 56 crates | No | **Fetches a browser and a driver program by itself** when no path is set, and panics if the fetch fails | | Refused: silent download |

Not looked at further: `playwright-rs` 0.21.0 drives Playwright's Node driver, which [decision 0004](../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md) already set aside; `chaser-oxide` describes itself as undetectable, which rule 5 forbids; `cdp-rs` (last release 2023) and `chrome-devtools-rs` (2021) are not maintained.

How each fact was established:

| Fact | How |
| --- | --- |
| Versions, dates, download counts, declared licences | Read from the crates.io interface on 2026-11-17 |
| Licence tree and crate counts | Run: `spikes/browser-base/measure.sh`; the output is in `spikes/browser-base/results/dependencies-and-licences.txt` |
| Pipe or port, default arguments, fetching, usage data, waits | Read in each crate's published source at the version named. Only `chromiumoxide` and `thirtyfour` were also run |
| The licence file of `auto_generate_cdp` | Read: its manifest says `license-file = "LICENSE.txt"`, and the file opens "GNU GENERAL PUBLIC LICENSE, Version 3" |

## What was measured

Three programs do the same steps against the test application's form for one line item: start the browser with a data directory the program owns, connect, open the page, wait for a named condition, read one field, read it 200 more times, take an accessibility-tree snapshot, intercept the save the page sends and fail it, close, and then start again and kill the browser while a call is pending. The spike's README lists the steps. Three runs each, interleaved, on four cores shared with other builds, so the times carry noise of tens of milliseconds. Lines are lines of code after formatting, without comments and blank lines.

| Measure | (a) Our own client, pipe | (b) `chromiumoxide`, port | (c) `thirtyfour`, driver program |
| --- | --- | --- | --- |
| Our code: protocol client | 166 lines | 0 | 0 |
| Our code: starting the browser with the pipe | 163 lines: 60 for POSIX, both ways; 66 for Windows; the rest shared | 0 | 0 |
| Our code: the steps | 265 lines, plus 93 for three extra measurements | 170 lines | 69 lines, and it stops at step 3 |
| Crates added | 7 | 44 | 30 |
| Clean release build, beyond the harness's 21 s | 3 s | 111 s, of which its generated protocol types take 55 s and the crate itself 45 s | 13 s |
| Start of the browser program to the first field value | 345, 359, 400 ms | 311, 351, 364 ms | Not reached |
| One read on a warm page, median of 200 | 0.14 to 0.21 ms | 0.11 to 0.21 ms | Not reached |
| Accessibility-tree snapshot, about 60 nodes | 2.5 to 2.7 ms | 3.2 to 3.7 ms, through the crate's raw command | The classic protocol has none |
| Intercepting the page's save and failing it | Done. The test application's control interface reports 0 writes stored | Done, through raw commands. 0 writes stored | The classic protocol has none |
| The wait | A protocol event, then an observer in an isolated world | The crate's navigation wait, then our own observer: the crate has no wait for an element | A poll |
| Close | The browser exits with status 0; no process is left | The same | Not reached |
| Browser killed while a call is pending | The call fails in 11 to 20 ms with "disconnected, request was sent". A later call fails at once with "disconnected, request was not sent". No process is left | The call fails in 7 to 12 ms with a channel error that does not say whether the request was sent. No process is left | Not reached |

Both handoffs of the pipe on POSIX gave the same times.

Three more measurements, with our own client only:

| Measure | Result | What it means |
| --- | --- | --- |
| Locate a text box by role and accessible name with `Accessibility.queryAXTree`, then read it: 4 round trips | Median 16.7 ms, 95th percentile 17.2 ms, in every run | The query appears to wait for a rendering frame: 16.7 ms is one frame at 60 per second. It fits the budget of [section 225](../spec/2-kernel/225-performance.md), but it is eighty times the cost of the helper |
| Locate the same field by its label with a helper in the page, and read it: 1 round trip | Median 0.19 ms | The injected helper should be the first way to locate, and the protocol's accessibility query the check or the fallback |
| A late element, 20 times: an observer in the page, against asking every 50 ms | The observer's callback runs under 0.1 ms after the element appears, and the answer is one round trip, about 0.2 ms, behind it. The poll found it 35 to 43 ms late, and can be anywhere from 0 to 50 | The bet of H-03 holds on this case. The re-render case was not measured |
| A second tab on the warm browser | Ready in 26 to 33 ms; opened, navigated, waited, and read in 55 to 73 ms | Inside the budget of 100 ms for a tab on a warm browser. A cold start to the first value, 0.35 to 0.40 s here, is inside the budget of 1 s |

## WebDriver

Run: the driver program on this machine is for major version 147, and the browser is 153. The driver refused the session: "This version of ChromeDriver only supports Chrome version 147. Current browser version is 153". A browser that updates itself therefore needs a new driver program each time, which the engine would have to fetch. The product does not fetch programs silently, and the browser already on a person's machine has no driver beside it.

Read, not run: the driver listens on a loopback port and has no sign-in of its own; the classic protocol has no accessibility tree and no way to intercept a request; `thirtyfour` waits for an element by asking again on an interval. The newer two-way protocol, as `rustenium` uses it, still goes through the driver program.

A matching driver was not run. The session's safety rules did not allow running a program fetched from the network, which is the same rule the product keeps.

## The debugging pipe on each operating system

How the browser takes the pipe was read in the browser's source on 2026-11-17: [the pipe handler](https://github.com/chromium/chromium/blob/main/content/browser/devtools/devtools_pipe_handler.cc), [where it is started](https://github.com/chromium/chromium/blob/main/content/browser/devtools/devtools_agent_host_impl.cc), and [the switches](https://github.com/chromium/chromium/blob/main/content/public/common/content_switches.cc).

| | Linux and macOS | Windows |
| --- | --- | --- |
| What the browser expects | `--remote-debugging-pipe`. It reads requests on file descriptor 3 and writes replies on file descriptor 4. One code path for every POSIX system | `--remote-debugging-pipe` and `--remote-debugging-io-pipes=<read>,<write>`: two pipe handles, written as unsigned integers. The browser relies on an inherited handle having the same value in the child as in the parent, checks that each is a pipe, and reads and writes it with ordinary blocking calls. Without the second switch it asks the C runtime for descriptors 3 and 4, which only a parent that fills a reserved field of the start-up record can provide |
| Messages | JSON, each ended by one NUL byte. A binary form exists and is not needed | The same |
| What the standard library and Tokio can do without `unsafe` | Give a child its standard input, output, and error, and nothing else. Putting a pipe on descriptor 3 needs a step between fork and exec, which is the `unsafe` function `pre_exec` | The same three, each copied as an inheritable handle whose value the caller never learns, so it cannot be named on the command line. Making a handle of our own inheritable needs one call to the operating system, `SetHandleInformation` |
| Smallest sound way | The `command-fds` crate, 0.3.3, Apache-2.0, last release 2026-04-10, 3.7 million recent downloads: a safe interface over `pre_exec` that also handles two descriptors that collide. The `unsafe` is inside it; none is ours | One `unsafe` call to `SetHandleInformation` through `windows-sys`, which is already in the tree. The call takes a handle we own and changes one flag |
| A way with no `unsafe` anywhere | Start `/bin/sh -c 'exec "$0" "$@" 3<&0 4>&1 0</dev/null 1>/dev/null'` with the browser and its arguments as a list, and give the shell two ordinary pipes as standard input and output. The shell moves them and becomes the browser, so the process identifier is the browser's | None found |
| Proven | **Run on Linux, both ways.** macOS is reasoned: the browser's code path and the helper crate are the same for both, and the code compiles for `aarch64-apple-darwin` | **Not run.** The code compiles for `x86_64-pc-windows-msvc`. The browser project's own driver hands the pipe over the same way, with the same switch ([its source](https://github.com/chromium/chromium/blob/main/chrome/test/chromedriver/net/pipe_builder.cc)) |

Three things about Windows that the design must hold:

- **Another child must not inherit the handles.** The standard library, at the pinned version 1.100, starts every child with handle inheritance on ([its source](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/process/windows.rs)). While the two handles are marked inheritable, any other process the engine starts at that moment would get them, and with them the signed-in browser. So the mark is set, the browser started, and the mark cleared under one lock that every process start in the engine holds. The supervisor is already the only thing that starts a process, so the lock is its. The browser project's own driver names the handles to inherit in a list, which the standard library offers only through an `unsafe` function that is not stable.
- **The browser's ends must be blocking handles.** The spike opens two named pipes with Tokio, keeps the server ends, which Tokio reads and writes without blocking, and opens the client ends with the standard library, which gives ordinary blocking handles. Each pipe allows one instance and one client, and the engine is that client before the browser starts. Whether the browser accepts these handles is the first thing to prove on Windows; the fallback is two anonymous pipes, read and written on two threads of their own.
- **Which browser version first took the switch is unverified.** It is in the current source and in [the 139 source that a distribution publishes](https://sources.debian.org/src/chromium/139.0.7258.127-1/chrome/test/chromedriver/net/pipe_builder.cc). The version is to be found before a minimum browser version is written down.

## Finding a browser

Read in `chromiumoxide`'s source, as a list of where it looks: on macOS, under `/Applications`, each as `<Name>.app/Contents/MacOS/<Name>`; on Windows, the registry key `App Paths\chrome.exe` and Edge under the 32-bit program-files folder; on Linux, `/opt/google/chrome` and names on the search path. The other places a browser is installed on Windows and Linux were not collected from a primary source and are owed.

The product cannot search the path: [section 204](../spec/2-kernel/204-supervising-subsystems.md) says a program is found at a declared place and never through a search path. It cannot fetch silently either. So the order is: a program named in settings; a browser installed by the explicit install action; then a fixed list of known places per operating system, kept as data.

The browser ignores the debugging switches when it runs on its default data directory ([section 302](../spec/3-packs/302-the-web-pack-browser-engine.md) already says so). The version that began this, recalled as 136, could not be checked today: the page that announced it could not be reached. Every launch in the spike passed `--user-data-dir` with a directory the program made, and the browser accepted the pipe.

## What CI needs

Read on 2026-11-17 in the lists of what the hosted build machines carry: the [Linux](https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Readme.md), [Windows](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md), and [macOS](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md) images each have Chrome and Edge installed, at version 156 or 157. So the engine's tests need no download in CI: they use the installed browser, found by the same fixed list the product uses.

Unverified: whether the browser's sandbox starts on the hosted Linux machine as it is. If it does not, the test harness alone may pass `--no-sandbox`, through a setting that exists only for tests, and the journal says so.

## Not done

- Nothing ran on Windows or macOS.
- No long session. The brief asked for stability over 30 minutes of continuous use; no program ran for more than a few seconds.
- No memory figure, and no Playwright sidecar to compare with, so H-01 is answered only in part: a direct client works and is small, and whether it is lighter and faster than a sidecar was not measured.
- `rustwright` was not run, because it sends usage data unless switched off and its source already ruled it out.
- Shadow roots, frames, re-renders, and actionability were not touched. Research brief 3 covers them.

## Sources

- Crates: [chromiumoxide](https://crates.io/crates/chromiumoxide), [headless_chrome](https://crates.io/crates/headless_chrome), [auto_generate_cdp](https://crates.io/crates/auto_generate_cdp), [chromey](https://crates.io/crates/chromey), [rustwright](https://crates.io/crates/rustwright), [rustwright-core](https://crates.io/crates/rustwright-core), [ferrous-browser](https://crates.io/crates/ferrous-browser), [thirtyfour](https://crates.io/crates/thirtyfour), [fantoccini](https://crates.io/crates/fantoccini), [webdriver](https://crates.io/crates/webdriver), [rustenium](https://crates.io/crates/rustenium), [command-fds](https://crates.io/crates/command-fds), [windows-sys](https://crates.io/crates/windows-sys).
- The standard library on Windows: [starting a process](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/process/windows.rs), and [its extensions](https://github.com/rust-lang/rust/blob/master/library/std/src/os/windows/process.rs), where turning inheritance off is marked stable from 1.101 and the handle list is `unsafe`.
- Inherited handles keep their value: [CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw), cited by the browser's own source.
