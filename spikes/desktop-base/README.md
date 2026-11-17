# Spike: what the desktop pack stands on

Roadmap package 8.1, with research brief 19. Throwaway code: it answers one question and is never promoted into the product. The answer is in [research note 19](../../docs/research/19-desktop-accessibility-base.md) and [decision 0120](../../docs/decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md).

**The question.** To find a control by role and name, read it, act on it, and wait on an event, what does the desktop pack stand on, on each operating system: one library over all three, a wrapper crate per system, or bindings of our own over each system's interface?

## What is here

| Folder | What | Run? |
| --- | --- | --- |
| `stand/window.py` | A small GTK 3 window: Description, Quantity, a password field, a check box, Save, and a status label. Save writes the form to a store file, which is the ground truth | Yes, on Linux, under a virtual X display |
| `stand-egui/` | The same window as one Rust program in egui, whose tree reaches each system through AccessKit. A candidate for test-application case 28 | Run on Linux. Type-checked for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` |
| `linux-atspi/` | AT-SPI over D-Bus with the `atspi` crates and `zbus`, on Tokio: find the application on the bus and its process from the bus daemon, the window, the field by role and name, set it, wait for its text-changed event, read it back, invoke Save, wait for the status label's event, ask the store | Yes, on Linux, against both windows |
| `windows-uia/` | The same steps over UI Automation through the `windows` crate alone | Type-checked for `x86_64-pc-windows-msvc`. Never run |
| `macos-ax/` | The same steps over AXUIElement and AXObserver through `objc2-application-services` | Type-checked for `aarch64-apple-darwin`. Never run |
| `checked-not-run/uiautomation/` | The Windows wrapper crate: a short program, type-checked | Type-checked for Windows only |
| `checked-not-run/xa11y/` | The one library over three systems: its tree, licences, and build only; its source read | No |
| `results/linux-atspi.txt` | What the Linux program printed: three runs against GTK, one against egui with the session's accessibility switch off, three with it on | |
| `run-linux.sh` | Starts a virtual display, a private session bus, the window, and the spike, and stops them | Yes |

## Running it

Each folder is a crate of its own, outside the product's workspace. Give the spike its own build folder.

```text
cd spikes/desktop-base/linux-atspi && CARGO_TARGET_DIR=../target cargo build
cd ../stand-egui && CARGO_TARGET_DIR=../target cargo build
cd .. && ./run-linux.sh                                   # the GTK window
STAND=egui SPIKE_ENABLE=1 SPIKE_UNTITLED=1 ./run-linux.sh # the egui window
cd windows-uia && CARGO_TARGET_DIR=../target cargo check --target x86_64-pc-windows-msvc
cd ../macos-ax && CARGO_TARGET_DIR=../target cargo check --target aarch64-apple-darwin
```

The Linux run needs `Xvfb`, `dbus-daemon`, `at-spi2-core`, and the system Python 3.12 with the GObject bindings for GTK 3; the egui window needs the X and Wayland client libraries. `SPIKE_ENABLE=1` switches on the session's accessibility setting, which egui needs and GTK does not. **That setting is the person's, and it is kept after the session ends**; reset it afterwards with `gsettings reset` on the key `toolkit-accessibility`. `SPIKE_UNTITLED=1` accepts the egui window, whose frame has no name. `SPIKE_DUMP=1` prints each node of the tree.

The licence check was run with a copy of the repository's `deny.toml` beside each `Cargo.toml`, because the checker found no other way to be given it; the copies were removed afterwards.
