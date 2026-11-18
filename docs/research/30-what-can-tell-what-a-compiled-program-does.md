# Research: what can tell what a compiled program does

**Status:** completed on 2026-11-18 from three surveys made that day, each from the projects' own pages: the interfaces a program and its operating system publish about it, and the tools that watch a program run; the analysis engines and decompilers, with the project studied in [note 16](16-rea-methods.md) compared as a dependency and as a source of code; and the Rust libraries that read compiled programs. Versions, dates and licences were read from repositories, release feeds and the npm, PyPI, NuGet, Maven Central and crates.io registries. Nothing was installed, built or run, and no program was started. Not verified, and marked **UNVERIFIED** wherever it matters: the pages of strace, Wireshark, Microsoft Learn, Frida, mitmproxy, sourceware, freedesktop, man7, rohitab, Chrome developer and the W3C (not reachable); the sites of the four commercial engines (search summaries only); the GitHub programming interface; any real compile on the three operating systems. Every figure called an estimate is the surveys' estimate for one experienced engineer, not a measurement. Feeds [decision 0276](../decisions/0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md), which holds the choices, under the purpose of [decision 0277](../decisions/0277-the-north-star-an-agent-uses-the-software-a-person-already-has.md).

## The table

Each row is one thing the scout might learn about a compiled program. The three middle columns say what each way would cost; "chosen" is what decision 0276 chose. The sections after the table hold the evidence.

| Capability | Use as a program the person installs | Take a part and adapt | Build our own | Chosen (0276) | Reason |
| --- | --- | --- | --- | --- | --- |
| **Published interfaces: Windows**, management-class schemas | `Get-CimClass` to JSON, part of Windows; days (est.) | Nothing useful | `wmi` 0.18.4 or `windows`; 600 to 1,000 lines, 1 to 2 weeks (est.) | Build (point 1) | States `Read` and `Write` per property and the privileges a method needs; asks the management service, not the provider |
| **Windows**, COM type libraries | OleView is interactive; no unattended tool found (**UNVERIFIED**) | `msft-typelib` (Apache-2.0, no dependencies, 3.6k lines), fuzzed by us; 3 to 5 days (est.) | `LoadTypeLibEx` through `windows`, 1,000 to 1,500 lines, 2 to 3 weeks (est.); or the file format from LGPL-only references, 3,000 to 4,000 lines, 4 to 8 weeks (est.) | Read as files (point 1); the means is open | States which members are property gets; a file read starts no server |
| **macOS**, scripting dictionary and property list | Apple's `sdef` for the older formats only | `plist` linked (MIT) | `.sdef` reader on quick-xml, 300 to 600 lines, 1 week (est.) | Build (point 1) | States property access; the property list names the dictionary and holds URL schemes, document types and services |
| **Linux**, D-Bus introspection | `busctl --auto-start=no` or `gdbus`; days (est.) | Nothing needed | The XML on quick-xml (about 300 lines) or `zbus`/`zbus_xml` (400 to 800 lines with the walker); 1 to 2 weeks (est.) | Build (point 1) | States property access; every call carries the no-auto-start flag so a service that is not running is not started |
| **Command metadata**, manual pages and shell completion files | fish's page converter is Python and GPL; `carapace` licence **UNVERIFIED** | Fig specs are ISC data, written by others and stale for the installed version; the GPL parsers cannot be copied | roff `mdoc` and `man`, plus zsh and fish line readers; 1,000 to 2,000 lines for roff and a few hundred each for the others, 3 to 5 weeks (est.) | Build, read as text (point 2) | No permissive Rust parser exists; a completion script is a program and is never run |
| **Command metadata**, PowerShell manifests and help | `pwsh` with auto-loading off and data cmdlets only; days (est.) | The PowerShell repository (MIT, C#) is not usable outside the .NET runtime | `.psd1` reader of about 300 lines, MAML help on quick-xml, attributes from .NET metadata; 2 to 3 weeks (est.) | Build, read as data (point 2) | `Get-Command` imports the module and runs its top-level code |
| **Plain facts of a binary**: kind, system, imports, exports | `llvm-readobj`, `rabin2 -j` (LGPL), `diec`; output shape drifts with versions; days (est.) | `object`'s modules, 8,000 to 12,000 lines; buys nothing the linked crate does not | Link `object` trimmed (one new package); or write 2,500 to 4,000 lines, 4 to 6 weeks (est.) | Link permissive libraries, or write where small (point 3) | One package, the most exercised reader in Rust, hostile-input cases already found |
| **Plain facts**: version resource, manifest, entitlements | `codesign -d --entitlements` (macOS) | `plist` readers, 1,400 to 1,900 lines | Version resource 300 to 500 lines on `object`; manifest about 50 lines; entitlements from `object`'s code-signature blobs and `plist` | Link or write (point 3) | Each is small and stable; no second parser of the same format is needed |
| **Signer** | `verifysign` asks the operating system (BSD-3-Clause); nothing on Linux | `authenticode`'s two files (546 lines) onto `cms` and `x509-cert` 0.3; 2 to 3 days (est.) | The certificate-table walk, about 150 lines; the ASN.1 and CMS parsing is months (est.) | Who signed, read (point 3) | Reading is ours; trusting a signature belongs to the operating system |
| **Managed-code metadata**: .NET | `ildasm`, `monodis` put a runtime on the machine; unstable text | `windows-metadata`'s reader, about 2,200 lines, hardened; 2 to 3 weeks (est.) | ECMA-335 tables, 1,500 to 4,000 lines, 3 to 8 weeks (est.) | Not decided | Both surveys lean to a small reader of our own or a hardened part; the choice is open |
| **Managed-code metadata**: Java classes and Android manifests | `jadx` decodes the manifest | `apk-info-axml` linked (Apache-2.0) | Class files 800 to 1,500 lines, 1 to 2 weeks (est.); DEX about 800 lines | Not decided | The class-file specification is precise and stable; no maintained DEX crate exists |
| **Decompiling native code** | Ghidra 12.2 headless (Apache-2.0); Kuna (Apache-2.0, one binary); weeks to bridge (est.) | The pure-Rust `ghidra-decompiler` crate, 164,518 lines, or Ghidra's C++ through bindings | A decompiler of our own: tens of thousands of lines, years (est.) | Use Ghidra, off by default (point 4); spike the Rust port (point 5); never build (point 10) | Largest answer for the least code here; the port is one day old and agent-written |
| **Decompiling .NET** | `ilspycmd` (MIT, needs a .NET runtime); bridge about 500 lines, days (est.) | A port of the decompiler: months (est.) | Metadata facts only, 3,000 to 6,000 lines (est.) | Use, the same form (point 4) | Source text is evidence for a reader, never an input to a run |
| **Decompiling Java and Android** | `jadx` (Apache-2.0, Java 11+); days (est.) | Ports of 60,000 to 90,000 lines of Java each: months (est.) | Class and DEX metadata, 3,000 to 6,000 lines, weeks (est.) | Use, the same form (point 4) | As above |
| **Firmware and archives** | `unblob` (MIT), `binwalk` v3 (MIT); both call outside extractors, Linux-centred | `binwalk`'s signature tables as data | Common formats on crates, 3,000 to 8,000 lines, weeks (est.) | Not decided | The long tail of vendor formats is the cost; nothing in 0276 reaches it |
| **Tracing a running program** | `strace` 7.3 (LGPL), Windows event tracing tools, `bpftrace` (Apache-2.0); 1 to 2 weeks each to drive and parse (est.) | `ferrisetw` (MIT OR Apache-2.0) for Windows event tracing; the LGPL decoders cannot be copied | Linux `ptrace` or eBPF, months; Windows event tracing about 1,500 lines (est.) | Left for later, not designed (point 11); injectors, debuggers on others' programs and system-wide trust roots excluded (point 9) | Passive tracing needs administrator rights; injection changes the program |
| **Traffic capture to an interface description** | `mitmproxy` 12.3.1 with `mitmproxy2swagger` 0.15.0 or `har-to-openapi` 3.0.1 (all MIT); days (est.) | Read `har-to-openapi`'s rules; port ideas, not code | A HAR reader and inference under our own rules, 1,500 to 2,500 lines, 3 to 5 weeks (est.) | Build on the existing drafter, one outside project read as reference (point 6); reading a saved capture later (point 11) | The inference is small everywhere and marks nothing read beyond the request method |
| **The studied project as a whole** | As an upstream MCP server: 189.6 MB unpacked, 255 tools, a tool surface that changes in 14 of 41 releases | Part by part: evidence rules already taken as method; its JavaScript analysis alone is about 41,400 lines, 3 to 6 months (est.), and churns | Its small checks and capture templating, when needed | Not included (point 7) | Nothing a program could link; 9 of 255 tools marked read-only; two licences this project does not take |

## Published interfaces, command metadata, and observation tools

The surveys sort every way of asking a program about itself into five kinds: **P** reads only what is published (a file, a registry entry, a manifest); **Q** asks the running program through its own published interface, which may start it; **R** runs the program; **A** attaches to or changes a running program; **I** intercepts its traffic. Decision 0276 point 1 stays within P, and Linux D-Bus introspection with the no-auto-start flag is Q for a service that is already running. The whole of point 2 is P, as text.

### Interfaces that state which calls only read

| Interface | What states the read | Library and licence | Status of the claim | Source |
| --- | --- | --- | --- | --- |
| Windows management classes | `Read`, `Write`, `Key`, `Privileges`, `WritePrivileges` qualifiers on properties and methods | `wmi` 0.18.4 and `windows` 0.62.2, both MIT OR Apache-2.0 | That a schema query does not invoke a provider is the survey's understanding, **UNVERIFIED** | [standard qualifiers](https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/WmiSdk/standard-qualifiers.md), [Get-CimClass](https://github.com/MicrosoftDocs/PowerShell-Docs/blob/main/reference/7.5/CimCmdlets/Get-CimClass.md) |
| COM type libraries | `FUNCDESC.invkind` separates `INVOKE_FUNC` from `INVOKE_PROPERTYGET`, `PROPERTYPUT` and `PROPERTYPUTREF` | `windows` 0.62.2; `msft-typelib` 0.2.0 (2026-10-10, Apache-2.0) | Whether loading a type-library resource from a DLL maps it as data only is **UNVERIFIED**; the COM registry layout is **UNVERIFIED** (Microsoft Learn not reachable) | [LoadTypeLibEx](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/oleauto/nf-oleauto-loadtypelibex.md), [FUNCDESC](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/oaidl/ns-oaidl-funcdesc.md) |
| macOS scripting dictionary (`.sdef`) | `access="r"` or `"rw"` on properties, named in the property list by `OSAScriptingDefinition` | `plist` 1.10.4 (2026-11-16, MIT); `sdef` 0.3.0 (MIT, 36 downloads) | Apps with dynamic terminology may answer only when sent an event: **UNVERIFIED**; `sdef(1)` and `sdef(5)` known through a mirror: **UNVERIFIED** | [OSAScriptingDefinition](https://developer.apple.com/documentation/bundleresources/information-property-list/osascriptingdefinition) |
| Linux D-Bus introspection | `access="read"`, `"write"` or `"readwrite"` on properties | `zbus` 6.3.0 (2026-11-13) and `zbus_xml` 6.0.0 (2026-10-16), both MIT | That a call to a name that is not running starts it unless the message carries the no-auto-start flag is **UNVERIFIED (search)**; dbus.freedesktop.org was not reachable | [busctl(1)](https://www.mankier.com/1/busctl) (mirror) |

Other interfaces the surveys found that state reads, and that decision 0276 does not take up: GraphQL introspection (`Query` against `Mutation`, asks a running server) ([specification](https://github.com/graphql/graphql-spec/blob/main/spec/Section%204%20--%20Introspection.md)); UI Automation's `Value.IsReadOnly` and macOS accessibility's settable attributes (both ask a running application, already the desktop pack's design). Interfaces that name entry points and say little about parameters: MSIX manifests, winget manifests, `.desktop` entries (the only Rust crates are MPL-2.0), URL schemes, systemd units, XPC service names. macOS App Intents metadata (`Metadata.appintents`): the format is undocumented and its contents are **UNVERIFIED**. Electron archives: a Chromium Pickle header, a JSON index and the data, readable in about 150 to 250 lines (est.); channel names found in scripts are inferred, never declared ([Electron fuses](https://github.com/electron/electron/blob/main/docs/tutorial/fuses.md)).

### Command metadata

| Software | Licence | Latest (date) | What rules it in or out | Source |
| --- | --- | --- | --- | --- |
| fish's `create_manpage_completions.py` | GPL-2.0 | part of fish; 2,245 lines | Cannot be copied into an MIT product | [fish-shell](https://github.com/fish-shell/fish-shell) |
| `@withfig/autocomplete` | ISC (npm) | 2.692.3 (2025-05-05) | Permissive data, hand-written by others: no evidence about the installed version | [inshellisense](https://github.com/microsoft/inshellisense) |
| `carapace-bin`, `carapace-spec` | **UNVERIFIED** (README fetched, licence file not) | **UNVERIFIED** | Not usable until the licence is read | [carapace-spec](https://github.com/carapace-sh/carapace-spec) |
| `clap_complete`, `clap_mangen` | MIT OR Apache-2.0 | 4.6.12 and 0.3.4 (2026-11-16) | Generators for Rust programs that already have the parser; not readers | crates.io |
| Cobra `__complete`, Click, `argcomplete` 3.7.2 (Apache-2.0), `shtab` 1.13.0 (MPL-2.0), `usage-lib` 7.4.0 (MIT) | as listed | as listed | Machine-readable, but the program is run to produce them (kind R); none says which calls only read | [Cobra](https://github.com/spf13/cobra/blob/main/site/content/completions/_index.md), [usage](https://github.com/jdx/usage) |
| `libmandoc-rs` | Apache-2.0 AND ISC AND BSD-2-Clause AND BSD-3-Clause | 0.14.0 (2026-11-14) | BSD-2-Clause is not on the list; vendored C | crates.io |
| `roff` | not stated by the survey | 1.1.1 | Writes roff only; no pure-Rust permissive roff parser was found | crates.io |
| PowerShell `Get-Command` | n/a | PowerShell 7.5 documents | Imports the module that holds the command; `$PSModuleAutoLoadingPreference = 'None'` stops that; `Import-PowerShellDataFile` reads a manifest as data | [Get-Command](https://github.com/MicrosoftDocs/PowerShell-Docs/blob/main/reference/7.5/Microsoft.PowerShell.Core/Get-Command.md) |

The zsh manual page and the mandoc project's page were not reachable (**UNVERIFIED**). Bash-completion files are loaded one per command name, and many call the target's own help to build their answer, so reading one as text yields only what is written literally.

### Tools that watch a program run

| Software | Licence | Latest (date) | What rules it in or out | Source |
| --- | --- | --- | --- | --- |
| Frida | LGPL-2.0-or-later WITH WxWindows-exception-3.1 | 17.27.0 (2026-11-17) | Injects an agent and rewrites code in the target; excluded by point 9 | [frida](https://github.com/frida/frida) |
| strace | LGPL-2.1-or-later | 7.3 (2026-10-22) | `-p` attaches and `--inject` changes calls; running a program the person starts under `--seccomp-bpf` is the allowed form | [strace](https://github.com/strace/strace) |
| ltrace | GPL (version of "or later" **UNVERIFIED**) | 0.8.1 per `configure.ac`; date **UNVERIFIED** | GPL; fragile on modern binaries | [ltrace](https://gitlab.com/cespedes/ltrace) |
| bpftrace, BCC | Apache-2.0 | v0.27.1 (24 Oct 2026, year inferred), v0.37.0 (2 Jul 2026, year inferred) | Root or capabilities; passive | [bpftrace](https://github.com/bpftrace/bpftrace), [bcc](https://github.com/iovisor/bcc) |
| DTrace on macOS | CDDL-1.0 **UNVERIFIED** | ships with the system | Needs System Integrity Protection relaxed from Recovery: a device protection (rule 5) | search summaries |
| DTrace on Windows | **UNVERIFIED** | MSI; version not stated | Needs boot configuration changed and possibly Secure Boot suspended | [driver documentation](https://github.com/MicrosoftDocs/windows-driver-docs/blob/staging/windows-driver-docs-pr/devtest/dtrace.md) |
| Event Tracing for Windows | part of the system | n/a | Passive; administrators, performance-log users or system accounts | [About ETW](https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/ETW/about-event-tracing.md) |
| Process Monitor | Proprietary; no redistribution or reverse engineering | 4.13 (2026-11-05) | Cannot be bundled or driven by the product; PML is closed | [terms](https://github.com/MicrosoftDocs/sysinternals/blob/main/sysinternals/license-terms.md) |
| API Monitor | Freeware, proprietary **UNVERIFIED** | 2.13.0.20210213 per a package list **UNVERIFIED** | Hooks calls inside the target | rohitab.com (not reachable) |
| macOS Endpoint Security | system | n/a | Needs an entitlement Apple grants (**UNVERIFIED**) | [Endpoint Security](https://developer.apple.com/documentation/endpointsecurity) |
| `gdb`, `lldb-dap`, WinDbg | GPL-3.0-or-later **UNVERIFIED**; Apache-2.0 WITH LLVM-exception **UNVERIFIED**; proprietary **UNVERIFIED** | gdb 18.1 (2026-09-25) **UNVERIFIED (search)**; LLVM 23.1.6 (2026-11-17); WinDbg **UNVERIFIED (search)** | Attach and stop threads; macOS refuses a target without `get-task-allow` | [lldb-dap](https://github.com/llvm/llvm-project/blob/main/lldb/tools/lldb-dap/README.md) |
| Wireshark and `tshark` | GPL-2.0-or-later | stable 4.6.9 and 4.4.19 (2026-09-23) | Reading a saved capture needs no privilege; live capture needs Npcap or capabilities | [tshark(1)](https://github.com/wireshark/wireshark/blob/master/doc/man_pages/tshark.adoc) |
| mitmproxy | MIT | 12.3.1 (2026-10-25) | TLS interception needs its authority trusted by the client; a trust root for the whole system lowers every program's protection, and pinned clients refuse it | [certificates](https://github.com/mitmproxy/mitmproxy/blob/main/docs/src/content/concepts/certificates.md) |
| `mitmproxy2swagger` | MIT (PyPI metadata; no licence file at the expected path) | 0.15.0 (2026-05-25) | 894 lines of Python; a person edits a path-template list between two passes; the schema of an array comes from its first element; the first observed body of an endpoint wins | [mitmproxy2swagger](https://github.com/alufers/mitmproxy2swagger) |
| `har-to-openapi` | MIT | 3.0.1 (2026-09-07) | `index.ts` 813 lines; merges scalar schemas across observations, parameterises numeric segments of a minimum length, filters auth headers; the better reference | [har-to-openapi](https://github.com/jonluca/har-to-openapi) |
| Optic | MIT | 1.0.9 (2025-08-10); repository archived 2026-01-12 | Unmaintained | [optic](https://github.com/opticdev/optic) |
| Postman Insights Agent | Apache-2.0 | **UNVERIFIED** | Sends what it learns to a vendor service; nothing may phone home | [repository](https://github.com/postmanlabs/postman-insights-agent) |
| Chrome DevTools Protocol | BSD-3-Clause (definitions) | `devtools-protocol` 0.0.1747637 (2026-11-18) | Since Chrome 136 remote debugging is ignored for the default profile directory (**UNVERIFIED (search)**); the product already owns a browser driver | [devtools-protocol](https://github.com/ChromeDevTools/devtools-protocol) |
| Playwright, Puppeteer | Apache-2.0 | 1.65.1 (2026-11-13) and 25.18.1 (2026-11-17) | Drive their own browser; the product has its own | [playwright](https://github.com/microsoft/playwright) |

Capture-to-description tools all do three things: group requests by method and path, turn identifier-looking segments into parameters, and infer a schema from bodies. None marks a call as read beyond its HTTP method; each is a draft for a person to check.

## Engines and decompilers

The two uses are different. Linking into the distributed MIT binary needs a licence on the closed list in `deny.toml` (MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0, Zlib, ISC, CDLA-Permissive-2.0). Starting a program the person installed and talking to it over its own interface places no terms on this product's code, so GPL and LGPL programs can be used that way.

| Software | Licence | Latest (date) | What rules it in or out | Source |
| --- | --- | --- | --- | --- |
| Ghidra | Apache-2.0 (bundles third-party parts under LGPL, GPL-with-Classpath, MPL-2.0 and others) | 12.2 (2026-10-22); 12.1.4 (2026-09-21) | In, as a program the person installs: headless (`analyzeHeadless`), all three systems, needs a 64-bit JDK 25 (12.1.x ran on JDK 21), about 1 GB; `pyghidraRun` offers to install `pyghidra` when missing, a network fetch | [Ghidra](https://github.com/NationalSecurityAgency/ghidra) |
| radare2 | LGPL-3.0, some GPL parts | 6.2.6 (2026-11-05) | Out to copy or link; usable only as an installed program; `r2pm` downloads plug-ins when asked | [radare2](https://github.com/radareorg/radare2) |
| rizin; Cutter | LGPL-3.0 with some GPL-3.0 files (split per file **UNVERIFIED**); GPL-3.0 | 0.9.1 (2026-06-29); 2.5.0 (2026-06-30) | Out to copy or link; Cutter has no headless mode stated | [rizin](https://github.com/rizinorg/rizin) |
| `rz-ghidra`, `r2ghidra` | LGPL-3.0-or-later; LGPL-3.0-only (the repository sidebar says GPL-3.0: the two disagree) | latest **UNVERIFIED** | Wrappers are LGPL although the Ghidra code inside is Apache-2.0: copy from Ghidra, not from them | READMEs |
| angr | BSD-2-Clause; its required lifter `pyvex` is BSD-2-Clause AND GPL-2.0-or-later | 10.0.7 (2026-11-13) | Out: Python, and a GPL part | [angr](https://github.com/angr/angr) |
| RetDec | MIT | v5.0 (2022-12-08); last commit 2026-05-26 | "Limited maintenance mode"; 5 to 6 GB | [retdec](https://github.com/avast/retdec) |
| Kuna | Apache-2.0 | v1.999 (2026-11-15) | A decompiler that is one binary, "originally ported from Ghidra", near-daily releases; a candidate beside Ghidra; not on crates.io | [kuna](https://github.com/Noelo-Lab/kuna) |
| `ghidra-decompiler` (crate) | Apache-2.0 | 2.1.0 (2026-09-25) | Pure Rust, 164,518 lines, no `unsafe`; output "textually identical" to the C++ decompiler on its corpus (89 data tests, 1,222 functions from 20 ELF binaries); three commits, all on 2026-09-25; its own text says it was ported by an agent; PE import tables and Mach-O stubs are not named | [crate](https://github.com/koute/ghidra-decompiler) |
| Binary Ninja, IDA, Hopper, JEB | Proprietary; all facts **UNVERIFIED** (vendor sites not reachable) | Binary Ninja 6.0 (2026-09-03); IDA 9.3 documented, 9.4 announced; Hopper 6.x; JEB 5.48 | Only on the person's own licence; headless use is limited to certain editions; never a default | search summaries |
| ILSpy, `ilspycmd` | MIT | 11.2 (2026-10-27) | In, as a program the person installs; needs a .NET runtime (version **UNVERIFIED**) | [ILSpy](https://github.com/icsharpcode/ILSpy) |
| dnSpyEx | GPL-3.0 | v6.6.0 (2026-06-20) | Out; no command line | [dnSpy](https://github.com/dnSpyEx/dnSpy) |
| JADX | Apache-2.0 | 1.5.6 (2026-07-10) | In, as an installed program; Java 11+; `jadx -d out --output-format json` | [jadx](https://github.com/skylot/jadx) |
| Apktool | Apache-2.0 | v3.0.3 (2026-07-20) | Decodes resources and smali; Java version **UNVERIFIED** | [Apktool](https://github.com/iBotPeaches/Apktool) |
| CFR; Procyon; Vineflower; Fernflower | MIT; Apache-2.0; Apache-2.0; Apache-2.0 | 0.152 (2021-12-11); v0.6.0 (2022-02-21); 1.12.0 (2026-04-29); ships inside IntelliJ Community | CFR and Procyon unmaintained; Vineflower (Java 17+) maintained | repositories |
| binwalk v3; unblob | MIT; MIT | v3.1.0 (2024-10-31); 26.6.4 (2026-06-04) | binwalk names 27 outside extractors and has had no release in two years; unblob targets Linux and Docker, Windows **UNVERIFIED** | [binwalk](https://github.com/ReFirmLabs/binwalk), [unblob](https://github.com/onekey-sec/unblob) |
| Detect It Easy | MIT | 4.0.0 (2026-11-04) | Signatures are scripts for its own engine; the `-j` flag was seen only in third-party wrappers (**UNVERIFIED**) | [DiE](https://github.com/horsicq/Detect-It-Easy) |
| YARA; YARA-X | BSD-3-Clause | v4.6.0 (2026-11-16, maintenance mode); v1.23.0 (2026-11-05) | `yara-x` depends on `wasmtime`, Apache-2.0 WITH LLVM-exception, which is not on the list as written | [yara-x](https://github.com/VirusTotal/yara-x) |
| LLVM object tools | Apache-2.0 WITH LLVM-exception | 23.1.6 (2026-11-17) | The exception expression is not on the list; JSON output ELF only (**UNVERIFIED**) | [llvm-project](https://github.com/llvm/llvm-project) |
| Capstone | BSD-3-Clause | 5.0.9 (2026-05-28) | A C library; the Rust binding bundles about 272,000 lines of C | [capstone](https://github.com/capstone-engine/capstone) |
| LIEF | Apache-2.0 | 1.0.0 (2026-07-12) | The Rust crate downloads a precompiled library at build time | [LIEF](https://github.com/lief-project/LIEF) |

Servers that already put these engines behind MCP (the Ghidra bridges, `pyghidra-mcp`, `radare2-mcp`, the IDA servers, `jadx-mcp-server`) are mostly a plug-in inside a window plus a bridge, and several offer writes (rename, patch) beside reads. Headless and read-only is the exception: `pyghidra-mcp` by default, `radare2-mcp`'s read-only mode, `idalib-mcp`. The Hex-Rays server's licence and tool list are **UNVERIFIED**.

## The studied project: depend on it, or copy from it

The project of [note 16](16-rea-methods.md), read now as the npm package `rea-agents` 6.9.0 (MIT, published 2026-11-12; the survey also gives 2026-11-11 once), unpacked and not run.

**Depend on it.** As an upstream server it is `npx -y rea-agents@6.9.0 mcp`; as a library it is not offered (no `main`, no `exports`).

| Measure | Value |
| --- | --- |
| Needs | Node.js ^22.23.4, ^24.22.1 or >=26.12.1, plus each engine's own program |
| Size | 41.9 MB download; 189.6 MB unpacked; 10,585 files; 175 packages in its shrinkwrap, 31 optional |
| Tools | 255; 9 declare `readOnlyHint`; 42 declare `destructiveHint`; 139 declare `openWorldHint` |
| Churn | 41 releases from 0.2.0 (2026-07-12) to 6.9.0 (2026-11-11), six major versions; 14 releases carry a "BREAKING CHANGES" section, including minor releases 6.1 to 6.9 |
| Licences of what it bundles | 134 MIT, 15 ISC, 10 Apache-2.0, 7 BSD-3-Clause, others permissive; two not permissive: `openpgp` (LGPL-3.0+, required) and `frida` (LGPL-2.0 WITH WxWindows-exception-3.1, optional); one BlueOak-1.0.0 |
| Tools a pass-through would have to filter | Process start and attach, a browser that can change application state, adb installs, traffic capture; version 3.2.0 removed its own ceilings |
| Startup time | not measured: **UNVERIFIED** |

**Copy from it, by part.** MIT terms: keep its copyright notice and the licence text with the copied or ported code.

| Part | Size (compiled JavaScript; estimate of source) | Churn (estimate) | Effort to port (est.) | Whose value |
| --- | --- | --- | --- | --- |
| JavaScript and bundle analysis | about 41,400 lines in 173 files, plus about 5,000 | about 270 commits a month | 3 to 6 months; needs a JavaScript parser in place of Babel | Its own logic |
| Web capture and capture-to-model templating | about 25,600; 4,000; 3,100 | about 270 for the browser part | templating 2 to 4 weeks; all, months | Its own logic; templating is small |
| Electron inspection | about 800 and 400 | about 30 | 1 to 2 weeks beyond the JavaScript analysis | Small |
| Evidence model and identity | about 4,800 | about 27 for `evidence.ts` | 1 to 3 weeks | Already adopted as method (decision 0053) |
| Bridge scripts | 6,713 (Java for Ghidra); 1,478; 1,330; about 1,199; 470; 1,107 | about 45 for the Ghidra bridge | 1 to 2 weeks to adapt a protocol | Run inside the other engine |
| Process ownership | about 15,200 and 3,000 | about 270 | 2 to 4 weeks, the rules not the code | Its own |
| Tool contracts | about 12,300 | about 280 | days, as reference | Already adopted as method |

Each of the paths took about 20 commits in two to three days in the survey's week. Counts are of compiled output in `dist/`; the repository's TypeScript is somewhat larger. By capability, the survey found that only web and JavaScript analysis and a few small parts are its own logic; native, managed and firmware work is glue around other people's engines.

**What follows.** A dependency puts 190 MB, a second runtime, two licences this project does not take and a tool surface that changes about every third release in front of every person who switched it on; each release would need an allow-list of read-only tools reviewed again, and only 9 of 255 tools say they read. A copy is worth the least where the churn is highest and the logic is its own and large. Decision 0276 point 7 takes neither: the method stays, the project is not started, bundled or recommended.

## Rust libraries

Versions and dates were read from crates.io on 2026-11-18. The licence check was run against the whole default-feature tree; `deny.toml` sets `all-features = true`, so the real gate is stricter than the survey's check.

### The smallest set that serves points 1 to 3 of decision 0276

| Crate | Version (date) | Licence | Use | Reason |
| --- | --- | --- | --- | --- |
| `object` | 0.41.0 (2026-11-10) | Apache-2.0 OR MIT | Kind, machine, imports, exports, delay imports, resource directory, and the code-signature blobs (entitlements, team identifier) of ELF, Mach-O, PE | One new package with trimmed features (`memchr` is in the tree); two owners; most exercised reader in Rust; no advisory; about 20 unsafe sites; breaks its interface on each 0.x release (74 versions) |
| `plist` | 1.10.4 (2026-11-16) | MIT | Property lists, XML and binary | Fuzzed; no advisory; shares `quick-xml` 0.42 with the workspace; one owner |
| `cms` and `x509-cert` | 0.2.3 (0.3.0-rc.2 on 2026-11-09); 0.3.0 | Apache-2.0 OR MIT | Reading who signed | `forbid(unsafe_code)`, fuzzed, many maintainers; take the two files of `authenticode` 0.6.0 (MIT OR Apache-2.0) onto 0.3, because 0.6.0 pins `cms` 0.2 and `digest` 0.10 and would duplicate packages |
| `verifysign` | 1.1.2 (2026-11-07) | BSD-3-Clause | Only if trust must be checked: asks the operating system (Windows, macOS) | Only the system knows catalog signatures and its trust store; about 81 unsafe sites for the calls; a revocation check may reach the network |
| `msft-typelib` | 0.2.0 (2026-10-10) | Apache-2.0 | COM type libraries in MSFT format | No dependencies, no unsafe found, 3.6k lines; 543 downloads, three versions, one publisher, no fuzz directory, no SLTG reader found: take it whole and fuzz it |
| `quick-xml` | 0.42.0 (workspace) | MIT | Manifests, `.sdef`, D-Bus introspection XML, MAML | Already in the workspace |
| `zbus` | 6.3.0 (workspace) | MIT | The live D-Bus call, with the no-auto-start flag | Already in the workspace (`Cargo.lock`); `zbus_xml` is not in it |

If scope widens: `windows-metadata` 0.100.0 (MIT OR Apache-2.0, no dependencies, built for trusted `.winmd`, has unwrap sites, so take its reader and harden it); `apk-info-axml` 1.1.1 (Apache-2.0, 11 packages); `prost-types` 0.14.6 (Apache-2.0); `gimli` 0.34.0 (MIT OR Apache-2.0, no required dependencies, in OSS-Fuzz), `pdb2` 0.11.0 and `symbolic-ppdb` 13.9.0 for debug information (never write these); the demanglers `rustc-demangle` 0.1.28, `cpp_demangle` 0.6.0 and `msvc-demangler` 0.11.0 (0, 2 and 3 dependencies); `cab` 0.6.0, `msi` 0.11.1 with `cfb` 0.16.0 (MIT; `msi` pulls `encoding_rs`, 137,000 lines). Measured by the survey: the whole recommended union is 92 packages, 40 not yet in `Cargo.lock` (335 today), about 507,000 lines, all on the licence list; binary size about 2 to 4.5 MB (estimate, no build was run), most of it debug information, MSI and WebAssembly, not the small readers.

Written here, by the survey's judgment, because they are small and stable: the Windows version resource (300 to 500 lines on `object`), the manifest (about 50), the key-file reader for `.desktop` and D-Bus `.service` files (200 to 300, the Rust crates are MPL-2.0), the D-Bus introspection reader (about 300 on `quick-xml`), the asar index (150 to 250), tar headers (200 to 300), and the `.sdef` reader (300 to 600). The workspace already has a bounded ZIP reader, `crates/a2m-sources/src/package.rs`, 290 lines.

### Avoid, and why

| Crate | Reason |
| --- | --- |
| `lief` | C++; its build script downloads a precompiled library over the network |
| `apple-codesign`, `apple-bundles`, `apple-xar`, `cryptographic-message-syntax`, `cpio-archive`, `freedesktop-desktop-entry`, `freedesktop_entry_parser` | MPL-2.0, not on the list; `apple-codesign` has 456 packages in its tree |
| `exe`, `macho2`, `dotnetdll`, `python_marshal` | GPL |
| `editpe` | BSD-2-Clause, not on the list; 32 packages |
| `yaxpeax-*`, `cafebabe` | 0BSD, not on the list |
| `pe-sign` | Non-standard licence file |
| `dotscope` | An MPL-2.0 dependency (`imbl`); 492,000 lines; 115 packages |
| `symbolic`, `symbolic-debuginfo` | MPL-2.0 dependencies and C `zstd`; 93 packages; `symbolic-demangle`'s default Swift feature compiles C++ |
| `capstone`, `zydis`, `bad64` | A C toolchain at build time |
| `yara-x` | `wasmtime` and `cranelift`, a compiler inside the process, 285 packages; `boreal` 1.4.0 (MIT OR Apache-2.0, 36 packages) is the pure-Rust alternative if matching is wanted |
| `binwalk`, `binwalk-ng` | A command-line tool; MPL dependencies; calls outside extractors |
| `zip` with default features | Off-list licences, C `zstd`, nine advisories in 2025 and 2026 (nearly all in extraction) |
| `sevenz-rust` | Unmaintained (RUSTSEC-2026-0246) and an unfixed path-traversal advisory (RUSTSEC-2026-0245) |
| `asar` | 89 packages: a command-line tree inside the library |
| `xmas-elf` | RUSTSEC-2025-0018 and weaker than `object` |
| `pdb`, `mach_object`, `dex`, `dexparser`, `hive-asar`, `debpkg` | No release since 2021 or 2022 |
| `pelite` | About 186 unsafe sites, one owner, no release since 2022-11-04; `object` and `goblin` cover the same ground |
| `wholesym` | Fetches from symbol servers; an MPL-2.0 dependency |
| `rpm` with default features | 201 packages and C `zstd` |
| `libmandoc-rs` | BSD-2-Clause and vendored C |

## What no existing software does

- **State, for a compiled program in general, which of its calls only read.** Help text never does. The four published interfaces of decision 0276's first point do for the programs that publish them; none exists for a command-line program as a whole. The capture-to-description tools mark nothing beyond the HTTP method, and 9 of 255 tools of the studied project say they only read.
- **Read a manual page or a shell completion file under a permissive licence in pure Rust.** No crate parses roff; none reads bash, zsh or fish completions.
- **Verify a signature to its trusted roots, or read a catalog signature, without asking the operating system.**
- **Read the older `aete` scripting resources, the SLTG form of a type library, a PyInstaller or Nuitka bundle, a XAR or `.pkg` under a permissive licence, or DEX with a maintained crate.**
- **Find protobuf descriptors embedded in a binary** with a crate in real use: `fdp-scan` has 80 downloads and `zarja-core` 328.
- **Say what a program exposes across all of the above.** The surveys found no project that joins them; the joining layer is ours.
- **Prove a write.** The studied project discovers and explains and never verifies a write; no engine or capture tool does.

## Where the surveys differ

- **Reading a type library.** The first survey builds on `LoadTypeLibEx` through the `windows` crate (1,000 to 1,500 lines); the third says writing the format ourselves is a mistake while `msft-typelib` exists, and takes or links that crate. The two mean different things by reading as a file: the first asks the operating system to load it, the second parses the bytes and so works on every operating system.
- **D-Bus.** The first survey builds on `zbus`; the third writes the XML reader on `quick-xml`, about 300 lines. `zbus` is in the workspace; `zbus_xml` is not.
- **Completion files.** The first survey reads zsh `_arguments` specs and fish lines as text; the third reads fish only and says bash and zsh cannot be read without running them.
- **.NET.** The second survey names `dotscope` for metadata; the third finds an MPL-2.0 dependency in its tree. The first leaves open whether `windows-metadata` reads general assemblies; the third says it parses the PE and CLI header itself and was not built for hostile input.
- **Licences off the list.** The second survey recommends `cafebabe` and `yaxpeax` (0BSD) and the Capstone binding; the third lists 0BSD and the C toolchain as reasons to avoid. Several licences the surveys met are not on the closed list as written: `Apache-2.0 WITH LLVM-exception` (`wasmtime`, LLVM), BSD-2-Clause, 0BSD, MPL-2.0, BlueOak-1.0.0, BSL-1.0, CC0 and MIT-0.
- **Tracing.** The first survey recommends offering `strace`, Windows event tracing and `bpftrace` as an aid the person runs; decision 0276 point 11 leaves that for later and undesigned. The first survey also allows interception with an authority trusted by one process the person started; decision 0276 point 9 excludes a trust root for the whole system and is silent on one process.
- **What `object` adds.** The third survey says in one place that linking `object` with trimmed features adds no package and in another that it adds one; the second is meant here, because `object` itself is new to the workspace and only `memchr` is already in it.
- **The studied project's date.** The second survey gives 6.9.0 as published 2026-11-12 in one place and 2026-11-11 in another.
- **Which interfaces state reads.** Decision 0276 says five. The surveys name four that it builds (management classes, type libraries, scripting dictionaries, D-Bus), and further ones that ask a running program (GraphQL, UI Automation, macOS accessibility).

## Could not determine

- Everything from the sites of Binary Ninja, IDA, Hopper and JEB; the Hex-Rays server's licence and tools; GNU binutils' version; `llvm-readobj`'s JSON output beyond ELF.
- Whether a schema query of the management service avoids invoking a provider; whether loading a type-library resource from a DLL maps only data; the COM registry layout; whether D-Bus names are started by a call without the no-auto-start flag, from the specification itself.
- The format of macOS `Metadata.appintents`; whether apps with dynamic terminology answer `sdef` without an event.
- Whether `carapace-bin`'s licence allows its use; the `diec` JSON flag; `ilspycmd`'s .NET version; Apktool's Java version; `unblob` on Windows and macOS; install sizes of radare2, JADX, angr, the .NET and Java runtimes and Ghidra's JDK.
- The date of `ltrace`'s release; the latest releases of `rz-ghidra`, `r2ghidra`, `radare2-mcp`, `pyghidra-mcp` and GhidraMCP; the Postman agent's version.
- The startup time of the studied project; line counts of its TypeScript (compiled output was counted); line counts of JADX and the ILSpy library.
- Whether `ghidra-decompiler` is equal to Ghidra on this project's own test programs (decision 0276 point 5; work package X7); whether Kuna's output is.
- Open issue counts of any Rust crate, docs.rs status, and a real compile of the recommended set on the three operating systems; the binary size of the set (estimated, not measured).
- Whether `object`'s new code-signature reader reads real signed programs of each version found; no program was read.
