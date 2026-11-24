//! What is checked about a program's file before it is ever started, on
//! every system, by every pack that starts one: that it is there, is a
//! program and not a script or a shell, and that nobody but the engine's
//! user, or the system's administrator, can replace it (decision 0250,
//! which moved this check here from the command pack, where decision 0216
//! built it).
//!
//! "Replace" covers the whole way from the declared path to the file. The
//! path is walked as the system would walk it when the program is started:
//! every folder on the way, every link, the folder each link is in, and
//! every folder on the way to where a link leads. Whoever can write one of
//! them chooses what the path names, whatever the file at its end is.
//!
//! On Unix the mode, the owner, and the group are read. On macOS an access
//! list beyond the mode is read too, and any entry in it that allows is a
//! refusal; on Linux the list is read where a group's write was accepted.
//! A group's write counts as the administrator's only for the groups
//! [`ProgramTrust`] holds. On Windows the owner and the
//! discretionary access list of each folder and of the file are read, and a
//! link, or any other reparse point, on the way is refused rather than
//! followed.
//!
//! The same walk proves a folder the engine made for a run closed to other
//! users ([`ProgramRole::Folder`]), and a file the engine wrote there, such
//! as the copy of a script a target keeps ([`ProgramRole::Script`]).
//!
//! Three more closed lists live here, beside the name list they qualify:
//! [`interpreter_form`], the one form in which each interpreter the product
//! starts reads a script file and takes the elements after it as data;
//! [`runs_its_own_code`], the commands of programs on the name list that
//! run no code but the program's own; and [`redirects_loading`], the
//! variables that make a program load code other than its own.

use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Why a program's file, or a folder or file the engine made, may not be
/// used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramRefused {
    /// The path is not absolute.
    NotAbsolute,
    /// Nothing is there, or it is not of the kind asked for, or it cannot
    /// be run.
    Missing,
    /// It is a shell, an interpreter, or a script one of them would run;
    /// or, asked for as an interpreter, one that reads its whole command
    /// line as code.
    Script,
    /// It, or the folder or link named, can be written by another user.
    Writable(PathBuf),
    /// Who may change it, or the folder named, could not be read: an access
    /// list that cannot be read, or holds an entry of a kind this check
    /// does not read, is a refusal, never a pass.
    Unreadable(PathBuf),
    /// A link, or another kind of reparse point, on the way: on Windows
    /// none is followed.
    #[cfg_attr(not(windows), allow(dead_code))]
    Link(PathBuf),
    /// A path on another machine's share or a device, or, on Windows, on a
    /// drive that is not a disk fixed in this machine, or by a drive letter
    /// that stands for a folder: its folders are not this machine's to
    /// vouch for, or not all of them are read.
    #[cfg_attr(not(windows), allow(dead_code))]
    Elsewhere,
}

/// What the path is checked as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramRole {
    /// A program that takes its arguments as data. A shell, an interpreter,
    /// or a program that starts another is refused by name, and so is a
    /// file that does not begin as this system's programs do, such as one
    /// that starts with `#!`.
    Program,
    /// The interpreter of a fixed pair: it is given a script the engine
    /// keeps, and the name list does not refuse it, except for the
    /// interpreters that read their whole command line as code, which are
    /// refused by name ([`reads_its_line_as_code`]). Every other check
    /// holds. Its arguments are held to [`interpreter_form`] apart.
    Interpreter,
    /// A program on the closed list of programs that start what their
    /// arguments name and have commands that run no code but their own
    /// ([`runs_its_own_code`], decision 0299, step one). The name list
    /// does not refuse it when its declared name, and the name of the file
    /// the path leads to, are both on that list; every other check holds.
    /// The caller holds every argument list it gives the program to one of
    /// the list's forms.
    OwnCode,
    /// A file the engine wrote, such as the copy of a kept script: it and
    /// every folder on the way are closed to other users. Its name, its
    /// first bytes, and whether it can be run are not looked at.
    Script,
    /// A folder the engine made: it and every folder on the way are closed
    /// to other users.
    Folder,
}

/// The groups whose write, on Unix, counts as the system's own, as the
/// administrator's does: a member could replace the file as the
/// administrator anyway.
///
/// On macOS the group with id 80, to which the system gives its
/// administrators, is always one: its members become the superuser through
/// the system's own prompt. On Linux no group is one by default: a group's
/// id says nothing about who is in it. Any other group is one only when the
/// caller names its numeric id; the command pack takes them from its
/// protected setting `packs.command.admin_groups`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramTrust {
    named: Vec<u32>,
}

/// The groups the system itself gives only to its administrators.
#[cfg(target_os = "macos")]
const SYSTEM_ADMIN_GROUPS: &[u32] = &[80];
#[cfg(not(target_os = "macos"))]
const SYSTEM_ADMIN_GROUPS: &[u32] = &[];

impl ProgramTrust {
    /// The system's own groups and those a person named.
    #[must_use]
    pub fn with_groups(named: Vec<u32>) -> Self {
        Self { named }
    }

    /// Whether a write by the group `id` counts as the system's own.
    #[cfg_attr(not(unix), allow(dead_code))]
    fn holds(&self, id: u32) -> bool {
        SYSTEM_ADMIN_GROUPS.contains(&id) || self.named.contains(&id)
    }
}

/// What the walk must find at the end of the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    File,
    Folder,
}

/// Checks the file or folder at `path`, as `role`. Blocking: it reads the
/// file system.
///
/// # Errors
///
/// The first reason found.
pub fn check_program(
    path: &Path,
    trust: &ProgramTrust,
    role: ProgramRole,
) -> Result<(), ProgramRefused> {
    if !path.is_absolute() {
        return Err(ProgramRefused::NotAbsolute);
    }
    let refused_name = |path: &Path| {
        let text = path.to_string_lossy();
        match role {
            ProgramRole::Program => is_shell_or_interpreter(&text),
            ProgramRole::Interpreter => reads_its_line_as_code(&text),
            ProgramRole::OwnCode => runs_its_own_code(&text).is_none(),
            ProgramRole::Script | ProgramRole::Folder => false,
        }
    };
    if refused_name(path) {
        return Err(ProgramRefused::Script);
    }
    let want = if role == ProgramRole::Folder {
        Want::Folder
    } else {
        Want::File
    };
    // Every folder and link from the declared path to the file itself.
    let real = os::walk(path, trust, want)?;
    let metadata = std::fs::metadata(&real).map_err(|_| ProgramRefused::Missing)?;
    if want == Want::Folder {
        return if metadata.is_dir() {
            Ok(())
        } else {
            Err(ProgramRefused::Missing)
        };
    }
    if !metadata.is_file() {
        return Err(ProgramRefused::Missing);
    }
    if refused_name(&real) {
        return Err(ProgramRefused::Script);
    }
    if role == ProgramRole::Script {
        return os::closed_file(&real, &metadata, trust);
    }
    // The first bytes, as many of the four as the file holds.
    let mut start = Vec::with_capacity(4);
    std::fs::File::open(&real)
        .and_then(|file| file.take(4).read_to_end(&mut start))
        .map_err(|_| ProgramRefused::Missing)?;
    // A script is never started as a program, nor as an interpreter: an
    // interpreter that is itself a script hands its own text to another.
    // A file is a program only when it starts as this system's programs
    // do; a text file without `#!` is a script too, which the C library
    // hands to the shell when the system will not run it.
    if !is_program_header(&start) {
        return Err(ProgramRefused::Script);
    }
    os::owned_by_us(&real, &metadata, trust)
}

/// How a file this system runs as a program begins (decision 0271): an
/// executable and linkable file on Linux and the other Unix systems, a
/// Mach-O file, thin in either byte order or universal, on macOS, and a
/// file with the DOS header that every Windows program has on Windows.
#[cfg(target_os = "macos")]
const PROGRAM_HEADERS: &[&[u8]] = &[
    &[0xfe, 0xed, 0xfa, 0xce],
    &[0xfe, 0xed, 0xfa, 0xcf],
    &[0xce, 0xfa, 0xed, 0xfe],
    &[0xcf, 0xfa, 0xed, 0xfe],
    &[0xca, 0xfe, 0xba, 0xbe],
    &[0xca, 0xfe, 0xba, 0xbf],
];
#[cfg(windows)]
const PROGRAM_HEADERS: &[&[u8]] = &[b"MZ"];
#[cfg(not(any(target_os = "macos", windows)))]
const PROGRAM_HEADERS: &[&[u8]] = &[&[0x7f, b'E', b'L', b'F']];

/// Whether `start`, the first bytes of a file, is how this system's
/// programs begin.
fn is_program_header(start: &[u8]) -> bool {
    PROGRAM_HEADERS
        .iter()
        .any(|header| start.starts_with(header))
}

/// The file names a program may not have, by kind: shells, interpreters
/// that take code as an argument or a file, and programs whose work is to
/// start the program, or run the code, their arguments name. Compared
/// without a `.exe` or `.com` suffix and without regard to case, as
/// [`refused_by_name`] says.
///
/// The list is a list of known names, and it can never be complete: any
/// program can be built to run what its arguments name, and many can be
/// told to through some option of their own. For a program whose name is
/// not on it, two things still hold: the digest pins the bytes of the
/// program that starts, and its arguments are the fixed ones in the
/// target's files, shown at review, with each typed value given to it as
/// one element of data. What such a program then loads or fetches because
/// of those fixed arguments is not pinned (decision 0271).
const SHELLS_AND_INTERPRETERS: &[&str] = &[
    // Shells.
    "sh",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "mksh",
    "csh",
    "tcsh",
    "fish",
    "ash",
    "busybox",
    "toybox",
    "cmd",
    "command",
    "powershell",
    "pwsh",
    "eval",
    "exec",
    // Interpreters that read code from an argument or a file.
    "wscript",
    "cscript",
    "mshta",
    "python",
    "pythonw",
    "ipython",
    "pypy",
    "perl",
    "ruby",
    "irb",
    "node",
    "nodejs",
    "deno",
    "bun",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "expect",
    "osascript",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "sed",
    "rscript",
    "r",
    "java",
    "javaw",
    "jshell",
    "jrunscript",
    "julia",
    "erl",
    "escript",
    "elixir",
    "iex",
    "swift",
    "kotlin",
    "scala",
    "groovy",
    "dart",
    "raku",
    "guile",
    "racket",
    "sbcl",
    "clojure",
    "clj",
    "runghc",
    "runhaskell",
    "ocaml",
    "mono",
    "csi",
    "fsi",
    "tsx",
    "ts-node",
    // Language launchers, package runners, and version managers: they
    // start an interpreter, or a package they fetch, by argument.
    "py",
    "pyw",
    "uv",
    "uvx",
    "pipx",
    "poetry",
    "pdm",
    "hatch",
    "conda",
    "mamba",
    "micromamba",
    "pixi",
    "npx",
    "npm",
    "pnpm",
    "pnpx",
    "yarn",
    "yarnpkg",
    "bunx",
    "corepack",
    "volta",
    "fnm",
    "nvm",
    "asdf",
    "mise",
    "pyenv",
    "rbenv",
    "nodenv",
    "bundle",
    "jbang",
    "rustup",
    "dotnet",
    // Container, sandbox, and compatibility runners: they start the
    // program, or the image, their arguments name.
    "docker",
    "podman",
    "nerdctl",
    "ctr",
    "kubectl",
    "flatpak",
    "snap",
    "firejail",
    "bwrap",
    "proot",
    "systemd-nspawn",
    "nix",
    "nix-shell",
    "wine",
    "wine64",
    "wsl",
    // Build tools: they run code the project they are pointed at holds.
    "make",
    "gmake",
    "nmake",
    "cmake",
    "ninja",
    "meson",
    "cargo",
    "go",
    "gradle",
    "gradlew",
    "mvn",
    "mvnw",
    "ant",
    "sbt",
    "msbuild",
    "bazel",
    "rake",
    // Programs that start the program their arguments name.
    "env",
    "xargs",
    "nohup",
    "sudo",
    "su",
    "doas",
    "pkexec",
    "runuser",
    "sg",
    "newgrp",
    "ssh",
    "nice",
    "ionice",
    "timeout",
    "stdbuf",
    "unbuffer",
    "chroot",
    "setsid",
    "setpriv",
    "setarch",
    "linux32",
    "linux64",
    "arch",
    "unshare",
    "nsenter",
    "taskset",
    "numactl",
    "prlimit",
    "chrt",
    "flock",
    "time",
    "watch",
    "script",
    "screen",
    "tmux",
    "xterm",
    "at",
    "batch",
    "run-parts",
    "find",
    "parallel",
    "systemd-run",
    "start-stop-daemon",
    "daemonize",
    "chpst",
    "faketime",
    "xdg-open",
    "gio",
    "strace",
    "ltrace",
    "valgrind",
    "gdb",
    "lldb",
    "perf",
    "dtruss",
    "caffeinate",
    "sandbox-exec",
    "xcrun",
    "open",
    "launchctl",
    "runas",
    "start",
    "explorer",
    "schtasks",
    "wmic",
    "forfiles",
    "pcalua",
    "conhost",
    // Windows programs that load or run the code their arguments name.
    "rundll32",
    "regsvr32",
    "regasm",
    "regsvcs",
    "installutil",
    "msiexec",
    "cmstp",
    "odbcconf",
    "control",
    "mmc",
    "hh",
    "msdt",
    "presentationhost",
    "infdefaultinstall",
];

/// Beginnings of the file names of the dynamic loaders of Linux, which run
/// the program their first argument names: `ld-linux-x86-64.so.2`,
/// `ld-musl-x86_64.so.1`, `ld.so`.
const LOADER_NAMES: &[&str] = &["ld-linux", "ld-musl", "ld.so", "ld64.so"];

/// The file name of a path, lower case, without a `.exe` or `.com` suffix,
/// and the same with a version taken off its end, as `python3.12` is
/// `python`.
fn stems(path: &str) -> (String, String, String) {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".exe")
        .or_else(|| lower.strip_suffix(".com"))
        .unwrap_or(&lower)
        .to_owned();
    let base = stem
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')
        .to_owned();
    (lower, stem, base)
}

/// Whether the file name `stem` is one of `names`: the name itself, the
/// name with a version at its end (`python3.12`), or, for a name of two
/// letters or more, the name followed by a version and anything after it
/// (`python3-dbg`, `perl5.36-thread`).
fn refused_by_name(names: &[&str], stem: &str, base: &str) -> bool {
    names.iter().any(|name| {
        stem == *name
            || base == *name
            || (name.len() >= 2
                && stem
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit() || c == '.')))
    })
}

/// Whether a program's path names a shell or an interpreter, a program
/// that starts another or runs the code its arguments name, or a script
/// file by its suffix.
#[must_use]
pub fn is_shell_or_interpreter(path: &str) -> bool {
    let (lower, stem, base) = stems(path);
    refused_by_name(SHELLS_AND_INTERPRETERS, &stem, &base)
        || LOADER_NAMES.iter().any(|name| stem.starts_with(name))
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".ps1")
        || lower.ends_with(".sh")
}

/// The command interpreters that read their whole command line as one
/// string, whatever elements it was given as: the Windows command
/// interpreter, its predecessor, and the script hosts. A typed value given
/// to one of them is re-read as part of a command. They are never started
/// as the interpreter of a pair, nor is a batch file, which the system
/// hands to the command interpreter.
const READS_ITS_LINE: &[&str] = &["cmd", "command", "wscript", "cscript", "mshta"];

/// Whether the path names a command interpreter that reads its whole
/// command line as code, or a batch file, which one of them runs.
#[must_use]
pub fn reads_its_line_as_code(path: &str) -> bool {
    let (lower, stem, base) = stems(path);
    READS_ITS_LINE.contains(&stem.as_str())
        || READS_ITS_LINE.contains(&base.as_str())
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
}

/// One interpreter the product starts as the half of a fixed pair, and the
/// one form in which it does: the exact fixed arguments, after which comes
/// the script's path and then the elements it takes as data.
struct Form {
    /// The names it is known by, as [`stems`] gives them.
    names: &'static [&'static str],
    /// The fixed arguments, element for element.
    args: &'static [&'static str],
}

/// The closed table of interpreters and their forms (decision 0250). An
/// interpreter is in it only where a test on a system that has it gives a
/// script values that hold the syntax of shells and interpreters and reads
/// back, byte for byte, what the script received: the POSIX shell on Linux
/// and macOS, Python on Linux and macOS and on Windows where it is installed
/// in a folder the check accepts, the JavaScript runtime on all three
/// systems, AppleScript on macOS, and Windows PowerShell. Every other interpreter is
/// refused until such a test proves its form.
///
/// Python and the JavaScript runtime are also the launchers most local MCP
/// servers are started through: a launcher given the file it loads, pinned
/// by digest at every start, is the same fixed pair (decision 0299, step
/// two). The digest pins that file and nothing it loads.
///
/// - The shell reads the file it is given and hands the rest to it as its
///   positional parameters. It takes no option: an option before the
///   file, such as `-c`, `-i`, or `-s`, changes what it reads.
/// - Python, isolated (`-I`): it reads no variable, no user folder, and not
///   the script's own folder, so nothing beside the script is loaded.
/// - Windows PowerShell with no profile, not interactive, and `-File`:
///   with `-File` what follows the script's path is bound to the script as
///   values; without it the rest of the line is a command.
/// - AppleScript's `osascript` reads the file and hands the rest to the
///   script's `run` handler.
/// - The JavaScript runtime reads its options up to the first element that
///   is not one, takes that as the file to run, and hands every element
///   after it, `--` among them, to the script as data. It takes no option:
///   `-e`, `-p`, and `-r` run or load code they name, and its variables
///   that do (`NODE_OPTIONS`, `NODE_PATH`) are refused by
///   [`redirects_loading`]. The path given is the engine's copy, which is
///   absolute, so it never begins with `-`.
const FORMS: &[Form] = &[
    Form {
        names: &["sh", "dash", "bash"],
        args: &[],
    },
    Form {
        names: &["python"],
        args: &["-I"],
    },
    Form {
        names: &["powershell"],
        args: &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ],
    },
    Form {
        names: &["osascript"],
        args: &[],
    },
    Form {
        names: &["node", "nodejs"],
        args: &[],
    },
];

/// Why an interpreter is not started with the fixed arguments it was
/// declared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpreterRefused {
    /// It reads its whole command line as code, or it is a batch file.
    ReadsItsLineAsCode,
    /// It is not an interpreter of the closed table: no test has proved the
    /// form in which it takes the elements after its script as data.
    NotKnown,
    /// It is in the table, and the fixed arguments are not its form, which
    /// is given.
    NotItsForm(&'static [&'static str]),
}

/// Whether the interpreter at `path`, given the fixed arguments `fixed`
/// before its script's path, reads that script as a file and takes every
/// element after it as data. Only the forms of the closed table pass, and
/// an empty list of fixed arguments passes only where the table says so.
///
/// # Errors
///
/// Why it is not started so.
pub fn interpreter_form(path: &str, fixed: &[String]) -> Result<(), InterpreterRefused> {
    if reads_its_line_as_code(path) {
        return Err(InterpreterRefused::ReadsItsLineAsCode);
    }
    let (_, stem, base) = stems(path);
    let form = FORMS
        .iter()
        .find(|form| form.names.contains(&stem.as_str()) || form.names.contains(&base.as_str()))
        .ok_or(InterpreterRefused::NotKnown)?;
    if fixed.len() == form.args.len()
        && fixed.iter().zip(form.args).all(|(given, its)| given == its)
    {
        Ok(())
    } else {
        Err(InterpreterRefused::NotItsForm(form.args))
    }
}

/// What kind of program, of those the name list refuses, a row of
/// [`runs_its_own_code`] is (decision 0299).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnCodeKind {
    /// A language launcher or runtime: given a file, it runs the file.
    Launcher,
    /// A version manager: it starts the version of a tool it chooses.
    VersionManager,
    /// A container runner: it starts the image its arguments name.
    ContainerRunner,
}

/// One command of a program that runs no code but the program's own: the
/// exact elements it is given, and whether names may follow them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnCodeForm {
    /// The elements, element for element, from the first.
    pub args: &'static [&'static str],
    /// Whether the elements may be followed by the end-of-options marker
    /// `--` and one or more values, each one element of data, such as the
    /// names of the containers to inspect. Nothing else may follow.
    pub names: bool,
}

/// One program of the closed list, by the names it is known by, and the
/// commands of it that run no code but its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnCode {
    /// Its kind.
    pub kind: OwnCodeKind,
    /// The names it is known by, as [`stems`] gives them; a version of
    /// digits and dots may follow a name, as `python3.12` follows `python`.
    pub names: &'static [&'static str],
    /// Its commands that run no code but its own. No other command of it
    /// is started.
    pub forms: &'static [OwnCodeForm],
}

/// A form of the elements alone, with nothing after them.
const fn only(args: &'static [&'static str]) -> OwnCodeForm {
    OwnCodeForm { args, names: false }
}

/// The closed list of the programs the name list refuses whose listed
/// commands run no code but the program's own (decision 0299, step one).
///
/// A command is on it only where the program, started as the supervisor
/// starts it (no variable but those named, in a fresh folder of its own),
/// was traced on a machine that had it and, given exactly those elements,
/// started no other program. A command that would run more is left off,
/// and some that look like reads are off for that reason:
///
/// - the version manager of Rust's toolchains, asked its own `--version`,
///   starts the compiler to report the compiler's version too; its listing
///   of toolchains and its home folder start nothing;
/// - the Python package manager's listing of interpreters starts each
///   interpreter it finds to ask what it is; its `--version` starts nothing;
/// - the container runner's help starts each of its plug-ins to ask what
///   it is, and its commands follow the context its user's files name,
///   which may reach the engine over a remote shell. Each listed command of
///   it names the context `default` first, which uses the local engine, and
///   asks for JSON by the word `json`, not by a template.
///
/// A program's own configuration, in its user's folders, can still make it
/// run more than this says (a container runner's plug-in hooks, for one).
/// The list holds for the program as it ships; the review shows the command.
const OWN_CODE: &[OwnCode] = &[
    OwnCode {
        kind: OwnCodeKind::Launcher,
        names: &["python"],
        forms: &[only(&["--version"]), only(&["-V"])],
    },
    OwnCode {
        kind: OwnCodeKind::Launcher,
        names: &["node", "nodejs"],
        forms: &[only(&["--version"]), only(&["-v"])],
    },
    OwnCode {
        kind: OwnCodeKind::Launcher,
        names: &["bun"],
        forms: &[only(&["--version"])],
    },
    OwnCode {
        kind: OwnCodeKind::VersionManager,
        names: &["uv"],
        forms: &[only(&["--version"])],
    },
    OwnCode {
        kind: OwnCodeKind::VersionManager,
        names: &["rustup"],
        forms: &[only(&["toolchain", "list"]), only(&["show", "home"])],
    },
    OwnCode {
        kind: OwnCodeKind::ContainerRunner,
        names: &["docker"],
        forms: &[
            only(&["--version"]),
            only(&["--context", "default", "version", "--format", "json"]),
            only(&[
                "--context",
                "default",
                "ps",
                "--all",
                "--no-trunc",
                "--format",
                "json",
            ]),
            only(&[
                "--context",
                "default",
                "images",
                "--no-trunc",
                "--format",
                "json",
            ]),
            OwnCodeForm {
                args: &["--context", "default", "container", "inspect"],
                names: true,
            },
            OwnCodeForm {
                args: &["--context", "default", "image", "inspect"],
                names: true,
            },
        ],
    },
];

/// The row of the closed list of programs whose listed commands run no
/// code but their own, for the program at `path`, by its file name: one of
/// the row's names, alone or followed by a version of digits and dots
/// (`python3`, `python3.12`), without a `.exe` or `.com` suffix and without
/// regard to case. A batch file, a file named with any other suffix, and
/// every other name have none.
#[must_use]
pub fn runs_its_own_code(path: &str) -> Option<&'static OwnCode> {
    if reads_its_line_as_code(path) {
        return None;
    }
    let (_, stem, _) = stems(path);
    OWN_CODE.iter().find(|row| {
        row.names.iter().any(|name| {
            stem.strip_prefix(name).is_some_and(|rest| {
                rest.chars().all(|c| c.is_ascii_digit() || c == '.')
                    && !rest.starts_with('.')
                    && !rest.ends_with('.')
            })
        })
    })
}

/// Names of variables that make a program load code other than its own, or
/// run code at its start, or run another program it names. Compared
/// without regard to case: names are so on Windows, and refusing more is
/// the safe side elsewhere. Each family says whose documentation it comes
/// from and why it is here (decision 0271).
const LOADING_NAMES: &[&str] = &[
    // The POSIX shell and bash, which the closed table of interpreters
    // starts: a file each sources at start (`ENV`), the options that make
    // it print a prompt that runs code, and the field separator, which old
    // shells took from their environment.
    "ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "PS4",
    "IFS",
    // The C library of Linux: its character-set converters are shared
    // objects loaded from `GCONV_PATH`; its tunables are read by the
    // dynamic loader itself; its locale and message files are data it
    // trusts as its own.
    "GCONV_PATH",
    "GLIBC_TUNABLES",
    "LOCPATH",
    "NLSPATH",
    // A program that runs "the shell" or "the command interpreter" runs
    // the one these name: `system()` of the C runtime on Windows runs
    // `COMSPEC`, and many programs on Unix run `SHELL`.
    "COMSPEC",
    "SHELL",
    // Helper programs a program runs by a variable: pagers, editors, the
    // prompts that ask for a password, and the commands of the version
    // control tool that programs often call.
    "PAGER",
    "MANPAGER",
    "LESSOPEN",
    "LESSCLOSE",
    "EDITOR",
    "VISUAL",
    "BROWSER",
    "SSH_ASKPASS",
    "SUDO_ASKPASS",
    "GIT_ASKPASS",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_EXEC_PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_PROXY_COMMAND",
    "GIT_TEMPLATE_DIR",
    // Node.js, and an Electron program told to behave as Node.js, which
    // then honours Node.js's variables.
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_REPL_EXTERNAL_MODULE",
    "ELECTRON_RUN_AS_NODE",
    // Java and the other runtimes of its machine.
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "CLASSPATH",
    // Ruby's gems, PHP's settings, Tcl's and Tk's start-up scripts and
    // packages, R's installation.
    "GEM_PATH",
    "GEM_HOME",
    "PHPRC",
    "PHP_INI_SCAN_DIR",
    "TCLLIBPATH",
    "TCL_LIBRARY",
    "TK_LIBRARY",
    "R_HOME",
    // Windows PowerShell runs on the .NET Framework, which loads assemblies
    // from `DEVPATH` when its configuration allows development mode.
    "DEVPATH",
    // AppleScript's `osascript`, which the closed table starts: the
    // folders it loads script libraries from.
    "OSA_LIBRARY_PATH",
    // Libraries that load modules as shared objects from folders a
    // variable names: GTK's and GIO's modules, Qt's plug-ins, GStreamer's
    // plug-ins, the authentication library's plug-ins, the graphics
    // library's drivers.
    "QT_PLUGIN_PATH",
    "QT_QPA_PLATFORM_PLUGIN_PATH",
    "SASL_PATH",
    "SASL_CONF_PATH",
    "LIBGL_DRIVERS_PATH",
    // Windows applies the compatibility layers this names to the program
    // and to what it starts.
    "__COMPAT_LAYER",
];

/// Beginnings of names of such variables, each a family whose every member
/// is refused:
///
/// - `LD_` and `DYLD_`: the dynamic loaders of Linux and macOS
///   (`LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`);
///   `__XPC_`: what the system's service launcher hands on as a loader
///   variable on macOS (`__XPC_DYLD_INSERT_LIBRARIES`).
/// - `BASH_`: bash's imported functions and its own variables (`BASH_ENV`).
/// - `PYTHON` and `__PYVENV`: every variable of Python (`PYTHONPATH`,
///   `PYTHONSTARTUP`, `PYTHONHOME`), and the one that tells it on macOS
///   which program it is.
/// - `PERL`, `RUBY`, `LUA_`, `JULIA_`, `ERL_`, `ELIXIR_`: every variable of
///   those runtimes, among them their module paths and the flags that run
///   code (`PERL5OPT`, `RUBYOPT`, `LUA_INIT`, `ERL_FLAGS`).
/// - `R_PROFILE`, `R_ENVIRON`, `R_LIBS`: R's start-up files and libraries.
/// - `COR_`, `CORECLR_`, `COMPLUS_`, `DOTNET_`, `COREHOST_`, and
///   `APPDOMAIN_MANAGER_`: the .NET Framework, on which Windows PowerShell
///   runs, and .NET: their profilers (`COR_PROFILER`,
///   `CORECLR_PROFILER_PATH`), every setting of the runtime
///   (`COMPlus_` and `DOTNET_`, among them `DOTNET_STARTUP_HOOKS`,
///   `DOTNET_ADDITIONAL_DEPS`, and `DOTNET_ROOT`), the host's own, and the
///   assembly and type the .NET Framework loads into every program as its
///   application domain manager (`APPDOMAIN_MANAGER_ASM`,
///   `APPDOMAIN_MANAGER_TYPE`).
/// - `PSMODULE`: the folders Windows PowerShell loads modules from by name
///   (`PSModulePath`), and its cache of what they hold.
/// - `OSA_`: AppleScript's libraries.
/// - `OPENSSL_`: OpenSSL's configuration, which names engines and
///   providers it loads as shared objects (`OPENSSL_CONF`,
///   `OPENSSL_ENGINES`, `OPENSSL_MODULES`), into any program linked with
///   it.
/// - `GTK_`, `GIO_`, `GST_PLUGIN`, `VK_`: the module paths of the desktop
///   toolkit and its input library, the media framework's plug-ins, and
///   the layers and drivers the graphics loader loads.
/// - `GIT_CONFIG`: settings given to the version control tool, some of
///   which name commands it runs.
const LOADING_PREFIXES: &[&str] = &[
    "LD_",
    "DYLD_",
    "__XPC_",
    "BASH_",
    "PYTHON",
    "__PYVENV",
    "PERL",
    "RUBY",
    "LUA_",
    "JULIA_",
    "ERL_",
    "ELIXIR_",
    "R_PROFILE",
    "R_ENVIRON",
    "R_LIBS",
    "COR_",
    "CORECLR_",
    "COMPLUS_",
    "DOTNET_",
    "COREHOST_",
    "APPDOMAIN_MANAGER_",
    "PSMODULE",
    "OSA_",
    "OPENSSL_",
    "GTK_",
    "GIO_",
    "GST_PLUGIN",
    "VK_",
    "GIT_CONFIG",
];

/// Whether a variable's name is one that makes a program load code other
/// than its own, run code at its start, or run another program it names.
/// The supervisor refuses a start that names one, and every pack refuses it
/// where the target's files are compiled. The list is closed and kept here
/// alone; it is a deny list, because a program's own settings come in
/// variables of every name. It is closed, and it is not complete: a
/// program can read any variable as the name of code to load. Its reasons
/// and limits are in decisions 0250 and 0271. `PATH` is not on it: the
/// supervisor holds each folder a `PATH` names to the check of a folder
/// instead.
#[must_use]
pub fn redirects_loading(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    LOADING_NAMES.contains(&upper.as_str())
        || LOADING_PREFIXES
            .iter()
            .any(|prefix| upper.starts_with(prefix))
}

#[cfg(unix)]
mod os {
    use std::ffi::OsString;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::{Component, Path, PathBuf};

    use super::{ProgramRefused, ProgramTrust, Want};

    const OTHERS_WRITE: u32 = 0o002;
    const GROUP_WRITE: u32 = 0o020;
    const STICKY: u32 = 0o1000;
    const ANY_EXECUTE: u32 = 0o111;
    /// How many links one path may lead through, as the system itself
    /// allows; more is a circle.
    const MOST_LINKS: u32 = 40;

    /// Whether the engine's user or the administrator owns it.
    fn ours(metadata: &std::fs::Metadata, us: u32) -> bool {
        metadata.uid() == us || metadata.uid() == 0
    }

    /// Whether only the engine's user or the administrator can change it.
    ///
    /// A file or folder a group may write is refused unless `trust` holds
    /// the group: who a group's members are is not known here, and a member
    /// who is not the engine's user is another user, unless the system, or
    /// the person in a protected setting, says the group is its
    /// administrators'.
    fn closed(metadata: &std::fs::Metadata, us: u32, folder: bool, trust: &ProgramTrust) -> bool {
        let mode = metadata.mode();
        if !ours(metadata, us) {
            return false;
        }
        // In a folder with the sticky mark, only an entry's owner may
        // replace it, so what we own there stays ours.
        let sticky = folder && mode & STICKY != 0;
        let group_closed = mode & GROUP_WRITE == 0 || trust.holds(metadata.gid());
        (mode & OTHERS_WRITE == 0 && group_closed) || sticky
    }

    /// The names of a path, last first, for a stack. `..` is kept as a
    /// name; the root and `.` are not names.
    fn names(path: &Path) -> impl Iterator<Item = OsString> {
        path.components().rev().filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::Prefix(_) | Component::RootDir | Component::CurDir => None,
        })
    }

    /// Walks an absolute path name by name, as the system does when it
    /// starts a program, and gives the path with no link left in it.
    ///
    /// Every folder entered must be closed to other users, and every link
    /// must be ours or the administrator's; where a link leads is walked
    /// the same way. So when this passes, nobody else can change what the
    /// declared path names between this check and a start. A folder at the
    /// end of the path is entered, and checked, as every other.
    pub(super) fn walk(
        declared: &Path,
        trust: &ProgramTrust,
        _want: Want,
    ) -> Result<PathBuf, ProgramRefused> {
        let us = rustix::process::getuid().as_raw();
        let root = PathBuf::from("/");
        let top = std::fs::metadata(&root).map_err(|_| ProgramRefused::Missing)?;
        if !closed(&top, us, true, trust) {
            return Err(ProgramRefused::Writable(root));
        }
        beyond_the_mode(&root, &top, trust)?;
        let mut at = root.clone();
        let mut ahead: Vec<OsString> = names(declared).collect();
        let mut links = 0;
        while let Some(name) = ahead.pop() {
            if name == ".." {
                // `at` holds no link, so its parent is the folder above it,
                // which was entered, and checked, on the way down.
                at.pop();
                continue;
            }
            let next = at.join(&name);
            let metadata = std::fs::symlink_metadata(&next).map_err(|_| ProgramRefused::Missing)?;
            if metadata.file_type().is_symlink() {
                links += 1;
                if links > MOST_LINKS {
                    return Err(ProgramRefused::Missing);
                }
                // The folder it is in was checked when it was entered. In a
                // folder with the sticky mark that is not enough: the link
                // itself must be ours.
                if !ours(&metadata, us) {
                    return Err(ProgramRefused::Writable(next));
                }
                beyond_the_mode(&next, &metadata, trust)?;
                let target = std::fs::read_link(&next).map_err(|_| ProgramRefused::Missing)?;
                if target.is_absolute() {
                    at.clone_from(&root);
                }
                ahead.extend(names(&target));
            } else if metadata.is_dir() {
                if !closed(&metadata, us, true, trust) {
                    return Err(ProgramRefused::Writable(next));
                }
                beyond_the_mode(&next, &metadata, trust)?;
                at = next;
            } else {
                // A file is the end of a path.
                if !ahead.is_empty() {
                    return Err(ProgramRefused::Missing);
                }
                at = next;
            }
        }
        Ok(at)
    }

    /// The file itself: it can be run, and nobody else can write it. Its
    /// folders were checked by [`walk`].
    pub(super) fn owned_by_us(
        real: &Path,
        metadata: &std::fs::Metadata,
        trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        if metadata.mode() & ANY_EXECUTE == 0 {
            return Err(ProgramRefused::Missing);
        }
        closed_file(real, metadata, trust)
    }

    /// The file itself: nobody else can write it.
    pub(super) fn closed_file(
        real: &Path,
        metadata: &std::fs::Metadata,
        trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        let us = rustix::process::getuid().as_raw();
        if !closed(metadata, us, false, trust) {
            return Err(ProgramRefused::Writable(real.to_path_buf()));
        }
        beyond_the_mode(real, metadata, trust)
    }

    /// What an access list beyond the mode grants, on macOS: an entry that
    /// allows anything, to anyone, is a refusal (decision 0271).
    #[cfg(target_os = "macos")]
    fn beyond_the_mode(
        path: &Path,
        _metadata: &std::fs::Metadata,
        _trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        super::macos::no_entry_allows(path)
    }

    /// What an access list beyond the mode grants, on Linux. There the
    /// group's bits of the mode are the most any named user or group of
    /// the list may hold, so a list adds nothing the mode does not show,
    /// except where the group's write was accepted because `trust` holds
    /// the group: the write may then be a named user's. A list is then a
    /// refusal (decision 0271).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn beyond_the_mode(
        path: &Path,
        metadata: &std::fs::Metadata,
        trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        if metadata.mode() & GROUP_WRITE == 0 || !trust.holds(metadata.gid()) {
            return Ok(());
        }
        match rustix::fs::lgetxattr_query(path, POSIX_ACCESS_LIST) {
            Err(rustix::io::Errno::NODATA | rustix::io::Errno::NOTSUP) => Ok(()),
            Ok(_) | Err(_) => Err(ProgramRefused::Unreadable(path.to_path_buf())),
        }
    }

    /// Where Linux keeps a file's access list beyond the mode.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) const POSIX_ACCESS_LIST: &str = "system.posix_acl_access";

    /// Other Unix systems: the mode alone is read.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
    fn beyond_the_mode(
        _path: &Path,
        _metadata: &std::fs::Metadata,
        _trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        Ok(())
    }
}

#[cfg(windows)]
#[path = "program/windows.rs"]
mod os;

#[cfg(target_os = "macos")]
#[path = "program/macos.rs"]
mod macos;

#[cfg(not(any(unix, windows)))]
mod os {
    //! A system that is neither: nothing is known of who may write a file
    //! there, so every program is refused (rule 7).

    use std::path::{Path, PathBuf};

    use super::{ProgramRefused, ProgramTrust, Want};

    pub(super) fn walk(
        declared: &Path,
        _trust: &ProgramTrust,
        _want: Want,
    ) -> Result<PathBuf, ProgramRefused> {
        Err(ProgramRefused::Unreadable(declared.to_path_buf()))
    }

    pub(super) fn owned_by_us(
        real: &Path,
        _metadata: &std::fs::Metadata,
        _trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        Err(ProgramRefused::Unreadable(real.to_path_buf()))
    }

    pub(super) fn closed_file(
        real: &Path,
        _metadata: &std::fs::Metadata,
        _trust: &ProgramTrust,
    ) -> Result<(), ProgramRefused> {
        Err(ProgramRefused::Unreadable(real.to_path_buf()))
    }
}

/// Keeps a folder the engine just made to the engine's user and the
/// system's own identities, on Windows: a protected access list that
/// grants the folder's owner, the local system account, and the built-in
/// Administrators group full control, and nobody else anything. On Linux
/// and macOS a private folder is made with the mode `0700` instead.
///
/// # Errors
///
/// When the access list could not be set.
#[cfg(windows)]
pub(crate) fn keep_folder_private(path: &Path) -> std::io::Result<()> {
    os::keep_private(path)
}

#[cfg(test)]
mod tests;
