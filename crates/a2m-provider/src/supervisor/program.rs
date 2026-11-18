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
//! On Unix the mode, the owner, and the group are read; access lists beyond
//! the mode are not. A group's write counts as the administrator's only for
//! the groups [`ProgramTrust`] holds. On Windows the owner and the
//! discretionary access list of each folder and of the file are read, and a
//! link, or any other reparse point, on the way is refused rather than
//! followed.
//!
//! The same walk proves a folder the engine made for a run closed to other
//! users ([`ProgramRole::Folder`]), and a file the engine wrote there, such
//! as the copy of a script a target keeps ([`ProgramRole::Script`]).
//!
//! Two more closed lists live here, because every pack that starts a
//! program needs them: [`interpreter_form`], the one form in which each
//! interpreter the product starts reads a script file and takes the
//! elements after it as data; and [`redirects_loading`], the variables that
//! make a program load code other than its own.

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
    #[cfg_attr(unix, allow(dead_code))]
    Unreadable(PathBuf),
    /// A link, or another kind of reparse point, on the way: on Windows
    /// none is followed.
    #[cfg_attr(not(windows), allow(dead_code))]
    Link(PathBuf),
    /// A path on another machine's share, or a device: its folders are not
    /// this machine's to vouch for.
    #[cfg_attr(not(windows), allow(dead_code))]
    Elsewhere,
}

/// What the path is checked as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramRole {
    /// A program that takes its arguments as data. A shell, an interpreter,
    /// or a program that starts another is refused by name, and so is a
    /// file that starts with `#!`.
    Program,
    /// The interpreter of a fixed pair: it is given a script the engine
    /// keeps, and the name list does not refuse it, except for the
    /// interpreters that read their whole command line as code, which are
    /// refused by name ([`reads_its_line_as_code`]). Every other check
    /// holds. Its arguments are held to [`interpreter_form`] apart.
    Interpreter,
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
pub fn check_program(path: &Path, trust: &ProgramTrust, role: ProgramRole) -> Result<(), ProgramRefused> {
    if !path.is_absolute() {
        return Err(ProgramRefused::NotAbsolute);
    }
    let refused_name = |path: &Path| {
        let text = path.to_string_lossy();
        match role {
            ProgramRole::Program => is_shell_or_interpreter(&text),
            ProgramRole::Interpreter => reads_its_line_as_code(&text),
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
    let mut start = [0_u8; 2];
    let read = std::fs::File::open(&real)
        .and_then(|mut file| file.read(&mut start))
        .map_err(|_| ProgramRefused::Missing)?;
    // A script is never started as a program, nor as an interpreter: an
    // interpreter that is itself a script hands its own text to another.
    if read == 2 && &start == b"#!" {
        return Err(ProgramRefused::Script);
    }
    os::owned_by_us(&real, &metadata, trust)
}

/// The file names a program may not have: shells, interpreters that take
/// code as an argument, and programs whose work is to start the program
/// their arguments name. Compared without a `.exe` suffix and without
/// regard to case.
///
/// The list is a net for a mistake, and it is not complete: many programs
/// can be told to run another through some option of their own. What
/// holds for those is the review of the target's files, where a command's
/// arguments are shown.
const SHELLS_AND_INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "csh",
    "tcsh",
    "fish",
    "ash",
    "busybox",
    "cmd",
    "command",
    "powershell",
    "pwsh",
    "wscript",
    "cscript",
    "python",
    "python2",
    "python3",
    "perl",
    "ruby",
    "node",
    "nodejs",
    "deno",
    "bun",
    "php",
    "lua",
    "tclsh",
    "osascript",
    "awk",
    "gawk",
    "sed",
    "env",
    "xargs",
    "nohup",
    "sudo",
    "su",
    "ssh",
    "eval",
    "exec",
    // More interpreters.
    "pythonw",
    "ipython",
    "rscript",
    "r",
    "java",
    "javaw",
    "jshell",
    "jrunscript",
    "irb",
    "wish",
    "expect",
    "mshta",
    // Programs that start the program their arguments name.
    "nice",
    "ionice",
    "timeout",
    "stdbuf",
    "chroot",
    "doas",
    "pkexec",
    "runuser",
    "setsid",
    "setpriv",
    "unshare",
    "nsenter",
    "taskset",
    "chrt",
    "flock",
    "time",
    "watch",
    "script",
    "strace",
    "ltrace",
    "find",
    "parallel",
    "systemd-run",
    "start-stop-daemon",
    "wsl",
    "rundll32",
    "forfiles",
];

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

/// Whether a program's path names a shell or an interpreter, a program
/// that starts another, or a script file by its suffix.
#[must_use]
pub fn is_shell_or_interpreter(path: &str) -> bool {
    let (lower, stem, base) = stems(path);
    SHELLS_AND_INTERPRETERS.contains(&stem.as_str())
        || SHELLS_AND_INTERPRETERS.contains(&base.as_str())
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
/// back, byte for byte, what the script received: the POSIX shell and
/// Python on Linux and macOS, AppleScript on macOS, and Windows PowerShell.
/// Every other interpreter is refused until such a test proves its form.
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
    if fixed.len() == form.args.len() && fixed.iter().zip(form.args).all(|(given, its)| given == its) {
        Ok(())
    } else {
        Err(InterpreterRefused::NotItsForm(form.args))
    }
}

/// Names of variables that make a program load code other than its own, or
/// run code at its start: the dynamic loaders' variables, and the start-up
/// and module-path variables of the language runtimes. Compared without
/// regard to case: names are so on Windows, and refusing more is the safe
/// side elsewhere.
const LOADING_NAMES: &[&str] = &[
    // The shells: a file each sources at start, functions it imports, and
    // the options that make it print a prompt that runs code.
    "ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "PS4",
    // The C library's character-set converters are loaded from here.
    "GCONV_PATH",
    // Node.js.
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_REPL_EXTERNAL_MODULE",
    // Java and the other runtimes of its machine.
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "CLASSPATH",
    // Ruby's gems, PHP's settings, Tcl's packages, R's start-up files.
    "GEM_PATH",
    "GEM_HOME",
    "PHPRC",
    "PHP_INI_SCAN_DIR",
    "TCLLIBPATH",
    "R_PROFILE",
    "R_PROFILE_USER",
    // PowerShell's modules, which load by name.
    "PSMODULEPATH",
    // The .NET runtime's start-up hooks.
    "DOTNET_STARTUP_HOOKS",
];

/// Beginnings of names of such variables: every variable of the dynamic
/// loader of Linux (`LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`) and of
/// macOS (`DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`), the shell's
/// imported functions and its own variables (`BASH_ENV`), every variable of
/// Python (`PYTHONPATH`, `PYTHONSTARTUP`, `PYTHONHOME`), Perl (`PERL5OPT`,
/// `PERL5LIB`), Ruby (`RUBYOPT`, `RUBYLIB`), and Lua (`LUA_INIT`,
/// `LUA_PATH`), and the profilers of the .NET runtime (`COR_PROFILER`,
/// `CORECLR_PROFILER_PATH`).
const LOADING_PREFIXES: &[&str] = &[
    "LD_", "DYLD_", "BASH_", "PYTHON", "PERL", "RUBY", "LUA_", "COR_", "CORECLR_",
];

/// Whether a variable's name is one that makes a program load code other
/// than its own, or run code at its start. The supervisor refuses a start
/// that names one, and every pack refuses it where the target's files are
/// compiled. The list is closed and kept here alone; it is a deny list,
/// because a program's own settings come in variables of every name, and
/// its reasons and limits are in decision 0250.
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
    pub(super) fn walk(declared: &Path, trust: &ProgramTrust, _want: Want) -> Result<PathBuf, ProgramRefused> {
        let us = rustix::process::getuid().as_raw();
        let root = PathBuf::from("/");
        let top = std::fs::metadata(&root).map_err(|_| ProgramRefused::Missing)?;
        if !closed(&top, us, true, trust) {
            return Err(ProgramRefused::Writable(root));
        }
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
                let target = std::fs::read_link(&next).map_err(|_| ProgramRefused::Missing)?;
                if target.is_absolute() {
                    at.clone_from(&root);
                }
                ahead.extend(names(&target));
            } else if metadata.is_dir() {
                if !closed(&metadata, us, true, trust) {
                    return Err(ProgramRefused::Writable(next));
                }
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
        Ok(())
    }
}

#[cfg(windows)]
#[path = "program/windows.rs"]
mod os;

#[cfg(not(any(unix, windows)))]
mod os {
    //! A system that is neither: nothing is known of who may write a file
    //! there, so every program is refused (rule 7).

    use std::path::{Path, PathBuf};

    use super::{ProgramRefused, ProgramTrust, Want};

    pub(super) fn walk(declared: &Path, _trust: &ProgramTrust, _want: Want) -> Result<PathBuf, ProgramRefused> {
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
/// and macOS a folder is made with the mode `0700`, and this does nothing.
///
/// # Errors
///
/// When the access list could not be set.
pub(crate) fn keep_folder_private(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        os::keep_private(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
