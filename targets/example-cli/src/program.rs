//! The program itself: what `example-cli` does with its arguments.
//!
//! It behaves like a business command-line tool. It parses typed arguments,
//! prints in several forms, uses both output streams, asks before it changes
//! anything, and says how it went by its exit status. The faults a test
//! switched on change that behavior at named points.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::control::{CHILD_POLL, CONTROL_FOLDER, ControlDir, Invocation};
use crate::faults::{FaultKind, StopHow, StopPoint, Stream};
use crate::store::{
    HomeLock, LOCATIONS, MAX_QUANTITY, PushRecord, SESSION_FILE, Store, StoredItem, WriteKind,
    WriteRecord,
};
use crate::{
    Build, CONFIRM_PROMPT, CONTROL_ENV, HOME_ENV, PASSWORD, PASSWORD_PROMPT, TOKEN_ENV, VERSION,
    exit,
};

/// The hidden command a child process of the program runs.
const LINGER: &str = "__linger";
/// How long a hung program waits to be killed before it gives up.
const HANG_LIMIT: Duration = Duration::from_secs(300);
/// How long the program waits for its child to say it is alive.
const CHILD_READY_WITHIN: Duration = Duration::from_secs(10);

/// Why a command ended without doing its work: the exit status, and the
/// message for standard error.
struct Stopped {
    code: u8,
    message: String,
}

impl Stopped {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn input(message: impl Into<String>) -> Self {
        Self::new(exit::INPUT, message)
    }

    fn internal(what: &str, error: &io::Error) -> Self {
        Self::new(exit::INTERNAL, format!("{what}: {error}"))
    }

    /// Standard output is gone. There is nobody to tell.
    fn silent() -> Self {
        Self::new(exit::INTERNAL, String::new())
    }
}

/// The arguments of one command, sorted into what they are.
struct Options {
    positional: Vec<String>,
    values: BTreeMap<&'static str, String>,
    switches: BTreeSet<&'static str>,
}

impl Options {
    /// Sorts `args` into positional arguments, options that take a value
    /// (`--name value` or `--name=value`), and switches. Everything after
    /// `--` is positional. An option the command does not know is an error.
    fn parse(
        args: &[String],
        valued: &[&'static str],
        switches: &[&'static str],
    ) -> Result<Self, Stopped> {
        let mut options = Self {
            positional: Vec::new(),
            values: BTreeMap::new(),
            switches: BTreeSet::new(),
        };
        let mut only_positional = false;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if only_positional {
                options.positional.push(arg.clone());
            } else if arg == "--" {
                only_positional = true;
            } else if let Some(body) = arg.strip_prefix("--") {
                let (name, inline) = match body.split_once('=') {
                    Some((name, value)) => (name, Some(value)),
                    None => (body, None),
                };
                if let Some(known) = switches.iter().find(|s| **s == name) {
                    if inline.is_some() {
                        return Err(Stopped::input(format!("--{name} takes no value")));
                    }
                    options.switches.insert(known);
                } else if let Some(known) = valued.iter().find(|v| **v == name) {
                    let value = match inline {
                        Some(value) => value.to_owned(),
                        None => args
                            .next()
                            .cloned()
                            .ok_or_else(|| Stopped::input(format!("--{name} needs a value")))?,
                    };
                    if options.values.insert(known, value).is_some() {
                        return Err(Stopped::input(format!("--{name} was given twice")));
                    }
                } else {
                    return Err(Stopped::input(format!("unknown option --{name}")));
                }
            } else if arg.len() > 1 && arg.starts_with('-') {
                return Err(Stopped::input(format!("unknown option {arg}")));
            } else {
                options.positional.push(arg.clone());
            }
        }
        Ok(options)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn required(&self, name: &str) -> Result<&str, Stopped> {
        self.value(name)
            .ok_or_else(|| Stopped::input(format!("--{name} is required")))
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }

    /// The one positional argument a command takes.
    fn one(&self, what: &str) -> Result<&str, Stopped> {
        match self.positional.as_slice() {
            [only] => Ok(only),
            [] => Err(Stopped::input(format!("{what} is required"))),
            _ => Err(Stopped::input("too many arguments")),
        }
    }

    fn none(&self) -> Result<(), Stopped> {
        if self.positional.is_empty() {
            Ok(())
        } else {
            Err(Stopped::input("too many arguments"))
        }
    }
}

// The argument types.

fn sku(text: &str) -> Result<&str, Stopped> {
    let bytes = text.as_bytes();
    let well_formed = bytes.len() == 7
        && bytes[..2].iter().all(u8::is_ascii_uppercase)
        && bytes[2] == b'-'
        && bytes[3..].iter().all(u8::is_ascii_digit);
    if well_formed {
        Ok(text)
    } else {
        Err(Stopped::input(format!(
            "{text:?} is not an item code; one looks like SK-1001"
        )))
    }
}

fn location(text: &str) -> Result<&str, Stopped> {
    let bytes = text.as_bytes();
    let well_formed = bytes.len() == 3
        && bytes[0].is_ascii_uppercase()
        && bytes[1..].iter().all(u8::is_ascii_digit);
    if well_formed {
        Ok(text)
    } else {
        Err(Stopped::input(format!(
            "{text:?} is not a location code; one looks like A01"
        )))
    }
}

fn whole_number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, Stopped> {
    if !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && let Ok(number) = text.parse()
    {
        return Ok(number);
    }
    Err(Stopped::input(format!(
        "{what} must be a whole number, not {text:?}"
    )))
}

fn item_name(text: &str) -> Result<&str, Stopped> {
    let length = text.chars().count();
    if (1..=60).contains(&length) && !text.chars().any(char::is_control) {
        Ok(text)
    } else {
        Err(Stopped::input(
            "a name is 1 to 60 characters with no control characters",
        ))
    }
}

fn reference(text: &str) -> Result<&str, Stopped> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-');
    if (1..=64).contains(&text.len()) && text.chars().all(allowed) {
        Ok(text)
    } else {
        Err(Stopped::input(
            "a reference is 1 to 64 letters, digits, dots, underscores, colons, or dashes",
        ))
    }
}

/// What the help command knows about one command.
struct Help {
    name: &'static str,
    usage: &'static str,
    about: &'static str,
    arguments: &'static [(&'static str, &'static str)],
}

const FORMAT_SHOW: (&str, &str) = ("--format <form>", "text (the default) or json");

const HELP: &[Help] = &[
    Help {
        name: "item show",
        usage: "item show <sku> [--format <form>]",
        about: "Show one item.",
        arguments: &[("<sku>", "Item code, such as SK-1001"), FORMAT_SHOW],
    },
    Help {
        name: "item list",
        usage: "item list [--location <code>] [--ref <reference>] [--format <form>]",
        about: "List items, as a table with tab-separated columns.",
        arguments: &[
            (
                "--location <code>",
                "Only items at this location, such as A01",
            ),
            (
                "--ref <reference>",
                "Only items created with this reference",
            ),
            (
                "--format <form>",
                "table (the default), json (one document), or jsonl (one document a line)",
            ),
        ],
    },
    Help {
        name: "item add",
        usage: "item add --name <name> --location <code> [--quantity <whole number>] [--ref <reference>] [--format <form>]",
        about: "Create an item. The program assigns its code. Running it twice creates two items.",
        arguments: &[
            ("--name <name>", "1 to 60 characters"),
            ("--location <code>", "Location code, such as A01"),
            ("--quantity <whole number>", "0 to 9999; 0 if absent"),
            (
                "--ref <reference>",
                "A reference of your own, to find the item by",
            ),
            FORMAT_SHOW,
        ],
    },
    Help {
        name: "item set",
        usage: "item set <sku> --quantity=<whole number> [--if-version <whole number>] [--yes]",
        about: "Set the quantity of one item. Asks \"Are you sure?\" unless --yes is given.",
        arguments: &[
            ("<sku>", "Item code, such as SK-1001"),
            ("--quantity <whole number>", "0 to 9999"),
            (
                "--if-version <whole number>",
                "Refuse unless the item still has this version",
            ),
            ("--yes", "Do not ask"),
        ],
    },
    Help {
        name: "item remove",
        usage: "item remove <sku> [--yes]",
        about: "Remove one item. Asks \"Are you sure?\" unless --yes is given.",
        arguments: &[
            ("<sku>", "Item code, such as SK-1001"),
            ("--yes", "Do not ask"),
        ],
    },
    Help {
        name: "catalog push",
        usage: "catalog push [--yes]",
        about: "Send the catalog outside. It cannot be taken back. Asks unless --yes is given.",
        arguments: &[("--yes", "Do not ask")],
    },
    Help {
        name: "report stock",
        usage: "report stock [--rows <whole number>] [--pause-ms <whole number>]",
        about: "Print a stock report row by row, with progress on standard error.",
        arguments: &[
            (
                "--rows <whole number>",
                "How many rows; one for each item if absent",
            ),
            (
                "--pause-ms <whole number>",
                "Milliseconds to wait after each row; 0 if absent",
            ),
        ],
    },
    Help {
        name: "login",
        usage: "login [--password-stdin]",
        about: "Sign in. Asks \"Password:\" unless --password-stdin is given.",
        arguments: &[(
            "--password-stdin",
            "Read the password from standard input without asking",
        )],
    },
    Help {
        name: "echo",
        usage: "echo [<anything>...]",
        about: "Print the arguments and the environment the program received, as JSON.",
        arguments: &[],
    },
];

const EXIT_HELP: &str = "Exit status:\n  0 ok   1 internal error   2 bad input   3 no such item   4 rejected\n  5 changed by someone else   6 busy   75 try again later   77 not signed in\n";

/// Whether the arguments ask for help with `--help`, before any `--`.
fn asks_for_help(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help")
}

/// The command the arguments mean, by the name faults and the invocation log use.
fn command_name(args: &[String]) -> String {
    let Some(first) = args.first() else {
        return "help".to_owned();
    };
    match first.as_str() {
        "echo" => "echo".to_owned(),
        "--version" | "version" => "version".to_owned(),
        "--help" | "help" => "help".to_owned(),
        "login" | "item" | "catalog" | "report" if asks_for_help(args) => "help".to_owned(),
        "login" => "login".to_owned(),
        "item" | "catalog" | "report" => match args.get(1) {
            Some(second) if !second.starts_with('-') => format!("{first} {second}"),
            _ => first.clone(),
        },
        _ => "unknown".to_owned(),
    }
}

/// Takes a leading `--home <dir>` or `--home=<dir>` off the arguments.
fn split_home(args: &[String]) -> (Option<PathBuf>, &[String]) {
    match args {
        [flag, dir, rest @ ..] if flag == "--home" => (Some(PathBuf::from(dir)), rest),
        [flag, rest @ ..] => match flag.strip_prefix("--home=") {
            Some(dir) => (Some(PathBuf::from(dir)), rest),
            None => (None, args),
        },
        [] => (None, args),
    }
}

fn control_dir() -> Option<ControlDir> {
    if let Some(named) = std::env::var_os(CONTROL_ENV) {
        return Some(ControlDir::new(PathBuf::from(named)));
    }
    let beside = std::env::current_exe().ok()?.parent()?.join(CONTROL_FOLDER);
    beside.is_dir().then(|| ControlDir::new(beside))
}

fn environment() -> BTreeMap<String, String> {
    std::env::vars_os()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// One start of the program.
struct Run {
    start: u64,
    command: String,
    control: Option<ControlDir>,
    faults: Vec<FaultKind>,
    home: Option<PathBuf>,
}

/// Runs the program with the process's own arguments and ends the process
/// with the exit status. `build` says which of the stand's two builds this is.
pub fn main(build: Build) -> ! {
    let all: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if all.first().map(String::as_str) == Some(LINGER) {
        linger(&all);
    }
    let (named_home, args) = split_home(&all);
    let home = named_home.or_else(|| std::env::var_os(HOME_ENV).map(PathBuf::from));
    let command = command_name(args);
    let control = control_dir();
    let mut run = Run {
        start: 0,
        command,
        control,
        faults: Vec::new(),
        home,
    };
    if let Some(control) = &run.control {
        let record = |start| Invocation {
            start,
            build: build.name().to_owned(),
            command: run.command.clone(),
            args: all.clone(),
            env: environment(),
            cwd: std::env::current_dir()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
            home: run
                .home
                .as_ref()
                .map(|home| home.to_string_lossy().into_owned()),
            exit: None,
            hung: false,
        };
        match control.begin(&run.command, record) {
            Ok((start, faults)) => {
                run.start = start;
                run.faults = faults;
            }
            Err(e) => {
                eprintln!("example-cli: error: the control folder cannot be used: {e}");
                std::process::exit(i32::from(exit::INTERNAL));
            }
        }
    }
    let code = match run.dispatch(args) {
        Ok(()) => exit::OK,
        Err(stopped) => {
            if !stopped.message.is_empty() {
                run.err(&format!("example-cli: error: {}\n", stopped.message));
            }
            stopped.code
        }
    };
    run.finish(code)
}

impl Run {
    fn fault<T>(&self, pick: impl Fn(&FaultKind) -> Option<T>) -> Option<T> {
        self.faults.iter().find_map(pick)
    }

    fn has(&self, kind: &FaultKind) -> bool {
        self.faults.contains(kind)
    }

    fn drifted(&self) -> bool {
        self.has(&FaultKind::Drift)
    }

    /// Prints on standard output, at once.
    fn out(&self, text: &str) -> Result<(), Stopped> {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(text.as_bytes())
            .and_then(|()| stdout.flush())
            .map_err(|_| Stopped::silent())
    }

    /// Prints on standard error. If nobody is reading, nothing is lost.
    fn err(&self, text: &str) {
        let _ = io::stderr().lock().write_all(text.as_bytes());
    }

    /// Ends the process with a status, and records the status first.
    fn finish(&self, code: u8) -> ! {
        let _ = io::stdout().lock().flush();
        if let Some(control) = &self.control {
            let _ = control.end(self.start, code);
        }
        std::process::exit(i32::from(code))
    }

    /// Stops here if a fault says so.
    fn stop_at(&self, point: StopPoint) {
        let how = self.fault(|fault| match fault {
            FaultKind::Stop { at, how } if *at == point => Some(*how),
            _ => None,
        });
        match how {
            None => {}
            Some(StopHow::Exit { code }) => self.finish(code),
            Some(StopHow::Abort) => {
                let _ = io::stdout().lock().flush();
                std::process::abort()
            }
            Some(StopHow::Hang) => {
                let _ = io::stdout().lock().flush();
                if let Some(control) = &self.control {
                    let _ = control.mark_hung(self.start);
                }
                let limit = Instant::now() + HANG_LIMIT;
                while Instant::now() < limit {
                    thread::sleep(Duration::from_millis(50));
                }
                self.finish(exit::HANG_ENDED)
            }
        }
    }

    /// What the faults do before any command starts its work.
    fn preamble(&self) -> Result<(), Stopped> {
        self.stop_at(StopPoint::Start);
        if let Some(ms) = self.fault(|fault| match fault {
            FaultKind::Delay { ms } => Some(*ms),
            _ => None,
        }) {
            thread::sleep(Duration::from_millis(ms));
        }
        if let Some(lifetime_ms) = self.fault(|fault| match fault {
            FaultKind::SpawnChild { lifetime_ms } => Some(*lifetime_ms),
            _ => None,
        }) {
            self.spawn_child(lifetime_ms)?;
        }
        for fault in &self.faults {
            match fault {
                FaultKind::Flood { stream, bytes } => self.flood(*stream, *bytes)?,
                FaultKind::Say {
                    stream: Stream::Stdout,
                    text,
                } => self.out(text)?,
                FaultKind::Say {
                    stream: Stream::Stderr,
                    text,
                } => self.err(text),
                _ => {}
            }
        }
        if self.has(&FaultKind::LeakSecret)
            && let Some(token) = std::env::var_os(TOKEN_ENV)
        {
            self.leak("token", &token.to_string_lossy())?;
        }
        if let Some(text) = self.fault(|fault| match fault {
            FaultKind::Prompt { text } => Some(text.clone()),
            _ => None,
        }) {
            self.ask(&text)?;
        }
        Ok(())
    }

    fn flood(&self, stream: Stream, bytes: u64) -> Result<(), Stopped> {
        let line = format!("{}\n", "x".repeat(63));
        for _ in 0..bytes.div_ceil(64) {
            match stream {
                Stream::Stdout => self.out(&line)?,
                Stream::Stderr => self.err(&line),
            }
        }
        Ok(())
    }

    fn leak(&self, what: &str, secret: &str) -> Result<(), Stopped> {
        let line = format!("debug: {what}={secret}\n");
        self.err(&line);
        self.out(&line)
    }

    /// Starts a child that keeps this program's output streams and outlives it.
    fn spawn_child(&self, lifetime_ms: u64) -> Result<(), Stopped> {
        let failed = |e: io::Error| Stopped::internal("a child could not be started", &e);
        let control = self
            .control
            .as_ref()
            .map(|control| control.path().as_os_str().to_owned())
            .unwrap_or_default();
        Command::new(std::env::current_exe().map_err(failed)?)
            .arg(LINGER)
            .arg(control)
            .arg(self.start.to_string())
            .arg(lifetime_ms.to_string())
            .stdin(Stdio::null())
            .spawn()
            .map_err(failed)?;
        if let Some(control) = &self.control {
            let limit = Instant::now() + CHILD_READY_WITHIN;
            while !control.child_is_ready(self.start) {
                if Instant::now() > limit {
                    return Err(Stopped::new(
                        exit::INTERNAL,
                        "the child did not say it was alive",
                    ));
                }
                thread::sleep(CHILD_POLL);
            }
        }
        Ok(())
    }

    /// Asks a question on standard output and reads the answer from standard
    /// input. Anything but `y` or `yes` stops the command with nothing changed.
    fn ask(&self, question: &str) -> Result<(), Stopped> {
        self.out(question)?;
        let mut answer = String::new();
        let _ = io::stdin().read_line(&mut answer);
        match answer.trim() {
            "y" | "Y" | "yes" => Ok(()),
            _ => Err(Stopped::input("not confirmed; nothing was changed")),
        }
    }

    fn confirm(&self, yes: bool) -> Result<(), Stopped> {
        if yes {
            Ok(())
        } else {
            self.ask(CONFIRM_PROMPT)
        }
    }

    fn home(&self) -> Result<&Path, Stopped> {
        let home = self.home.as_deref().ok_or_else(|| {
            Stopped::input(format!(
                "no home folder; give --home <dir> first, or set {HOME_ENV}"
            ))
        })?;
        std::fs::create_dir_all(home)
            .map_err(|e| Stopped::internal("the home folder cannot be made", &e))?;
        Ok(home)
    }

    /// The home folder of a command that reads or writes data, once the
    /// program is known to be signed in if it has to be.
    fn data_home(&self) -> Result<&Path, Stopped> {
        let home = self.home()?;
        if self.has(&FaultKind::RequireSession) && !signed_in(home) {
            return Err(Stopped::new(
                exit::SESSION_EXPIRED,
                "not signed in, or the session has expired; run: example-cli login",
            ));
        }
        Ok(home)
    }

    fn load(&self, home: &Path) -> Result<Store, Stopped> {
        Store::load(home).map_err(|e| Stopped::internal("the data file cannot be read", &e))
    }

    /// One write, in the order every write follows: take the lock, read, make
    /// the change, write the temporary file, commit by renaming, release.
    fn write<T>(
        &self,
        home: &Path,
        change: impl FnOnce(&mut Store) -> Result<T, Stopped>,
    ) -> Result<T, Stopped> {
        let failed = |e: io::Error| Stopped::internal("the data file cannot be written", &e);
        let lock = HomeLock::try_take(home)
            .map_err(failed)?
            .ok_or_else(|| Stopped::new(exit::BUSY, "another example-cli is writing; try later"))?;
        let mut store = self.load(home)?;
        let result = change(&mut store)?;
        store.write_temp(home).map_err(failed)?;
        self.stop_at(StopPoint::BeforeCommit);
        Store::commit(home).map_err(failed)?;
        drop(lock);
        self.stop_at(StopPoint::AfterCommit);
        Ok(result)
    }

    fn dispatch(&self, args: &[String]) -> Result<(), Stopped> {
        self.preamble()?;
        let after = |words: usize| args.get(words..).unwrap_or_default();
        match self.command.as_str() {
            "help" => self.help(args),
            "version" => self.version(),
            "echo" => self.echo(after(1)),
            "login" => self.login(after(1)),
            "item show" => self.item_show(after(2)),
            "item list" => self.item_list(after(2)),
            "item add" => self.item_add(after(2)),
            "item set" => self.item_set(after(2)),
            "item remove" => self.item_remove(after(2)),
            "catalog push" => self.catalog_push(after(2)),
            "report stock" => self.report_stock(after(2)),
            _ => Err(Stopped::input(format!(
                "no such command: {}; see example-cli --help",
                args.join(" ")
            ))),
        }
    }

    fn version(&self) -> Result<(), Stopped> {
        let version = self
            .fault(|fault| match fault {
                FaultKind::ReportVersion { version } => Some(version.clone()),
                _ => None,
            })
            .unwrap_or_else(|| VERSION.to_owned());
        self.out(&format!("example-cli {version}\n"))
    }

    fn help(&self, args: &[String]) -> Result<(), Stopped> {
        let words: Vec<&str> = args
            .iter()
            .map(String::as_str)
            .filter(|word| !matches!(*word, "help" | "--help"))
            .collect();
        if args.is_empty() {
            self.err("example-cli: error: a command is required\n");
            self.err(&overview());
            return Err(Stopped::new(exit::INPUT, String::new()));
        }
        if words.is_empty() {
            return self.out(&overview());
        }
        let name = words.join(" ");
        let known: Vec<&Help> = HELP
            .iter()
            .filter(|help| {
                help.name == name
                    || help
                        .name
                        .strip_prefix(name.as_str())
                        .is_some_and(|rest| rest.starts_with(' '))
            })
            .collect();
        match known.as_slice() {
            [] => Err(Stopped::input(format!("no such command: {name}"))),
            [one] if one.name == name => {
                let mut text = format!("Usage: example-cli {}\n\n{}\n", one.usage, one.about);
                if !one.arguments.is_empty() {
                    text.push_str("\nArguments:\n");
                    for (argument, meaning) in one.arguments {
                        text.push_str(&format!("  {argument:<28} {meaning}\n"));
                    }
                }
                text.push('\n');
                text.push_str(EXIT_HELP);
                self.out(&text)
            }
            group => {
                let mut text =
                    format!("Usage: example-cli {name} <command> [arguments]\n\nCommands:\n");
                for help in group {
                    text.push_str(&format!("  {:<14} {}\n", help.name, help.about));
                }
                self.out(&text)
            }
        }
    }

    fn echo(&self, args: &[String]) -> Result<(), Stopped> {
        let received = json!({
            "args": args,
            "env": environment(),
            "cwd": std::env::current_dir()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
        self.out(&format!("{received}\n"))
    }

    fn login(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &[], &["password-stdin"])?;
        options.none()?;
        let home = self.home()?;
        if !options.switch("password-stdin") {
            self.err(PASSWORD_PROMPT);
        }
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        let password = line.trim_end_matches(['\r', '\n']);
        if password.is_empty() {
            return Err(Stopped::input("no password was given"));
        }
        if self.has(&FaultKind::LeakSecret) {
            self.leak("password", password)?;
        }
        if password != PASSWORD {
            return Err(Stopped::new(exit::REJECTED, "sign-in refused"));
        }
        let session = json!({"signed_in_by_start": self.start});
        std::fs::write(home.join(SESSION_FILE), session.to_string())
            .map_err(|e| Stopped::internal("the session cannot be kept", &e))?;
        self.out("signed in\n")
    }

    fn item_show(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &["format"], &[])?;
        let sku = sku(options.one("an item code")?)?;
        let form = options.value("format").unwrap_or("text");
        if !matches!(form, "text" | "json") {
            return Err(Stopped::input(format!("unknown format {form:?}")));
        }
        let store = self.load(self.data_home()?)?;
        let item = store.items.get(sku).ok_or_else(|| absent(sku))?;
        if form == "json" {
            return self.out(&format!("{}\n", self.item_json(item)));
        }
        let quantity = if self.drifted() { "qty" } else { "quantity" };
        let mut text = format!(
            "sku: {}\nname: {}\n{quantity}: {}\nlocation: {}\nref: {}\nversion: {}\n",
            item.sku,
            item.name,
            item.quantity,
            item.location,
            item.reference.as_deref().unwrap_or("-"),
            item.version,
        );
        if let Some(note) = &item.note {
            text.push_str(&format!("note: {note}\n"));
        }
        self.out(&text)
    }

    /// One item as a JSON document. Drift renames the quantity.
    fn item_json(&self, item: &StoredItem) -> Value {
        let mut document = serde_json::to_value(item).unwrap_or(Value::Null);
        if self.drifted()
            && let Some(fields) = document.as_object_mut()
            && let Some(quantity) = fields.remove("quantity")
        {
            fields.insert("qty".to_owned(), quantity);
        }
        document
    }

    fn item_list(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &["location", "ref", "format"], &[])?;
        options.none()?;
        let at = options.value("location").map(location).transpose()?;
        let with_reference = options.value("ref").map(reference).transpose()?;
        let form = options.value("format").unwrap_or("table");
        if !matches!(form, "table" | "json" | "jsonl") {
            return Err(Stopped::input(format!("unknown format {form:?}")));
        }
        let store = self.load(self.data_home()?)?;
        let items: Vec<&StoredItem> = store
            .items
            .values()
            .filter(|item| at.is_none_or(|at| item.location == at))
            .filter(|item| {
                with_reference.is_none_or(|wanted| item.reference.as_deref() == Some(wanted))
            })
            .collect();
        let text = match form {
            "json" => {
                let documents: Vec<Value> = items.iter().map(|item| self.item_json(item)).collect();
                format!("{}\n", Value::Array(documents))
            }
            "jsonl" => items
                .iter()
                .map(|item| format!("{}\n", self.item_json(item)))
                .collect(),
            _ if self.drifted() => {
                let mut table = "sku\tname\tqty\tlocation\tversion\n".to_owned();
                for item in items {
                    table.push_str(&format!(
                        "{}\t{}\t{}\t{}\t{}\n",
                        item.sku, item.name, item.quantity, item.location, item.version
                    ));
                }
                table
            }
            _ => {
                let mut table = "sku\tname\tquantity\tlocation\n".to_owned();
                for item in items {
                    table.push_str(&format!(
                        "{}\t{}\t{}\t{}\n",
                        item.sku, item.name, item.quantity, item.location
                    ));
                }
                table
            }
        };
        self.out(&text)
    }

    /// The quantity to store for the quantity asked for.
    fn stored_quantity(&self, asked: u32) -> u32 {
        match self.fault(|fault| match fault {
            FaultKind::RoundQuantity { multiple } if *multiple > 0 => Some(*multiple),
            _ => None,
        }) {
            Some(multiple) => asked - asked % multiple,
            None => asked,
        }
    }

    fn quantity(text: &str) -> Result<u32, Stopped> {
        let quantity: u32 = whole_number(text, "a quantity")?;
        if quantity > MAX_QUANTITY {
            return Err(Stopped::new(
                exit::REJECTED,
                format!("quantity {quantity} is more than a bin holds ({MAX_QUANTITY})"),
            ));
        }
        Ok(quantity)
    }

    fn item_add(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(
            args,
            &["name", "location", "quantity", "ref", "format"],
            &[],
        )?;
        options.none()?;
        let name = item_name(options.required("name")?)?;
        let at = location(options.required("location")?)?;
        let with_reference = options.value("ref").map(reference).transpose()?;
        let form = options.value("format").unwrap_or("text");
        if !matches!(form, "text" | "json") {
            return Err(Stopped::input(format!("unknown format {form:?}")));
        }
        let quantity = options
            .value("quantity")
            .map(Self::quantity)
            .transpose()?
            .unwrap_or(0);
        if !LOCATIONS.contains(&at) {
            return Err(Stopped::new(
                exit::REJECTED,
                format!("there is no location {at}"),
            ));
        }
        let home = self.data_home()?;
        let stored_name = if self.has(&FaultKind::UppercaseName) {
            name.to_uppercase()
        } else {
            name.to_owned()
        };
        let created = self.write(home, |store| {
            let item = StoredItem {
                sku: format!("SK-{:04}", store.next_number),
                name: stored_name,
                quantity: self.stored_quantity(quantity),
                location: at.to_owned(),
                reference: with_reference.map(str::to_owned),
                note: None,
                version: 1,
            };
            store.next_number += 1;
            store.writes.push(WriteRecord {
                start: self.start,
                sku: item.sku.clone(),
                kind: WriteKind::Create,
            });
            store.items.insert(item.sku.clone(), item.clone());
            Ok(item)
        })?;
        // The program reports what it was asked for, as programs do.
        let shown = StoredItem {
            name: name.to_owned(),
            quantity,
            ..created
        };
        if form == "json" {
            self.out(&format!("{}\n", self.item_json(&shown)))
        } else if self.drifted() {
            self.out(&format!("{} created\n", shown.sku))
        } else {
            self.out(&format!("created {}\n", shown.sku))
        }
    }

    fn item_set(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &["quantity", "if-version"], &["yes"])?;
        let sku = sku(options.one("an item code")?)?;
        let quantity = Self::quantity(options.required("quantity")?)?;
        let if_version: Option<u64> = options
            .value("if-version")
            .map(|text| whole_number(text, "a version"))
            .transpose()?;
        let home = self.data_home()?;
        self.confirm(options.switch("yes"))?;
        self.write(home, |store| {
            let item = store.items.get_mut(sku).ok_or_else(|| absent(sku))?;
            if if_version.is_some_and(|expected| expected != item.version) {
                return Err(Stopped::new(
                    exit::STALE,
                    format!("{sku} has version {}; it was changed", item.version),
                ));
            }
            item.quantity = self.stored_quantity(quantity);
            item.version += 1;
            store.writes.push(WriteRecord {
                start: self.start,
                sku: sku.to_owned(),
                kind: WriteKind::Update,
            });
            Ok(())
        })?;
        if self.drifted() {
            self.out(&format!("{sku} updated (quantity {quantity})\n"))
        } else {
            self.out(&format!("updated {sku} quantity={quantity}\n"))
        }
    }

    fn item_remove(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &[], &["yes"])?;
        let sku = sku(options.one("an item code")?)?;
        let home = self.data_home()?;
        self.confirm(options.switch("yes"))?;
        self.write(home, |store| {
            store.items.remove(sku).ok_or_else(|| absent(sku))?;
            store.writes.push(WriteRecord {
                start: self.start,
                sku: sku.to_owned(),
                kind: WriteKind::Remove,
            });
            Ok(())
        })?;
        if self.drifted() {
            self.out(&format!("{sku} removed\n"))
        } else {
            self.out(&format!("removed {sku}\n"))
        }
    }

    fn catalog_push(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &[], &["yes"])?;
        options.none()?;
        let home = self.data_home()?;
        self.confirm(options.switch("yes"))?;
        let count = self.write(home, |store| {
            let count = store.items.len();
            store.pushes.push(PushRecord {
                start: self.start,
                count,
            });
            Ok(count)
        })?;
        if self.drifted() {
            self.out(&format!("{count} items pushed\n"))
        } else {
            self.out(&format!("pushed {count} items\n"))
        }
    }

    fn report_stock(&self, args: &[String]) -> Result<(), Stopped> {
        let options = Options::parse(args, &["rows", "pause-ms"], &[])?;
        options.none()?;
        let store = self.load(self.data_home()?)?;
        let rows: usize = options
            .value("rows")
            .map(|text| whole_number(text, "a number of rows"))
            .transpose()?
            .unwrap_or(store.items.len());
        let pause: u64 = options
            .value("pause-ms")
            .map(|text| whole_number(text, "a pause"))
            .transpose()?
            .unwrap_or(0);
        if rows > 0 && store.items.is_empty() {
            return Err(Stopped::new(exit::REJECTED, "there are no items to report"));
        }
        self.out(if self.drifted() {
            "row\tsku\tqty\n"
        } else {
            "row\tsku\tquantity\n"
        })?;
        for (index, item) in store.items.values().cycle().take(rows).enumerate() {
            let row = index + 1;
            self.out(&format!("{row}\t{}\t{}\n", item.sku, item.quantity))?;
            self.err(&format!("progress {row}/{rows}\n"));
            if pause > 0 {
                thread::sleep(Duration::from_millis(pause));
            }
        }
        self.err("done\n");
        Ok(())
    }
}

fn absent(sku: &str) -> Stopped {
    Stopped::new(exit::ABSENT, format!("no such item: {sku}"))
}

/// Whether the program is signed in: it kept a session, or was given the token.
fn signed_in(home: &Path) -> bool {
    home.join(SESSION_FILE).exists()
        || std::env::var_os(TOKEN_ENV).is_some_and(|token| token == PASSWORD)
}

fn overview() -> String {
    let mut text = format!(
        "example-cli {VERSION}\nInventory for the imaginary order-entry system `example`.\n\n\
         Usage: example-cli [--home <dir>] <command> [arguments]\n\nCommands:\n"
    );
    for help in HELP {
        text.push_str(&format!("  {:<14} {}\n", help.name, help.about));
    }
    text.push_str(
        "\nOptions:\n  --home <dir>   The folder that holds the data; or set EXAMPLE_CLI_HOME\n  \
         --help         Show this, or the help of one command: example-cli help item set\n  \
         --version      Show the version\n\n",
    );
    text.push_str(EXIT_HELP);
    text
}

/// What a child of the program does: lives until told to end, or until its
/// lifetime is over, holding a lock that shows it is alive.
fn linger(args: &[String]) -> ! {
    let control = args
        .get(1)
        .filter(|path| !path.is_empty())
        .map(|path| ControlDir::new(PathBuf::from(path)));
    let start: u64 = args.get(2).and_then(|text| text.parse().ok()).unwrap_or(0);
    let lifetime: u64 = args.get(3).and_then(|text| text.parse().ok()).unwrap_or(0);
    let held = control
        .as_ref()
        .and_then(|control| control.hold_child_file(start).ok());
    let limit = Instant::now() + Duration::from_millis(lifetime);
    while Instant::now() < limit {
        if control.as_ref().is_some_and(ControlDir::children_must_stop) {
            break;
        }
        thread::sleep(CHILD_POLL);
    }
    drop(held);
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn a_command_has_one_name() {
        let name = |words: &[&str]| command_name(&strings(words));
        assert_eq!(name(&["item", "set", "SK-1001", "--yes"]), "item set");
        assert_eq!(name(&["item", "set", "--help"]), "help");
        assert_eq!(name(&["item"]), "item");
        assert_eq!(name(&["--version"]), "version");
        assert_eq!(name(&["help", "item", "set"]), "help");
        assert_eq!(name(&["echo", "--help"]), "echo");
        assert_eq!(name(&["item", "set", "--", "--help"]), "item set");
        assert_eq!(name(&[]), "help");
        assert_eq!(name(&["sell"]), "unknown");
    }

    #[test]
    fn options_take_both_spellings_and_stop_at_the_marker() {
        let args = strings(&[
            "SK-1001",
            "--quantity=12",
            "--if-version",
            "3",
            "--yes",
            "--",
            "-x",
        ]);
        let Ok(options) = Options::parse(&args, &["quantity", "if-version"], &["yes"]) else {
            panic!("the arguments are good");
        };
        assert_eq!(options.positional, ["SK-1001", "-x"]);
        assert_eq!(options.value("quantity"), Some("12"));
        assert_eq!(options.value("if-version"), Some("3"));
        assert!(options.switch("yes"));
    }

    #[test]
    fn a_bad_argument_is_bad_input() {
        let refused = |words: &[&str]| {
            Options::parse(&strings(words), &["quantity"], &["yes"])
                .err()
                .map(|stopped| stopped.code)
        };
        assert_eq!(refused(&["--colour", "red"]), Some(exit::INPUT));
        assert_eq!(refused(&["-q"]), Some(exit::INPUT));
        assert_eq!(refused(&["--quantity"]), Some(exit::INPUT));
        assert_eq!(
            refused(&["--quantity=1", "--quantity=2"]),
            Some(exit::INPUT)
        );
        assert_eq!(refused(&["--yes=1"]), Some(exit::INPUT));
        assert!(sku("SK-1001").is_ok());
        assert!(sku("sk-1001").is_err());
        assert!(sku("SK-10010").is_err());
        assert!(location("A01").is_ok());
        assert!(location("A1").is_err());
        assert!(whole_number::<u32>("12", "n").is_ok());
        assert!(whole_number::<u32>("+12", "n").is_err());
        assert!(whole_number::<u32>("1.5", "n").is_err());
        assert!(item_name("Étiquette « fragile »").is_ok());
        assert!(item_name("two\tcolumns").is_err());
        assert!(reference("run:42.a_b-c").is_ok());
        assert!(reference("a b").is_err());
    }

    #[test]
    fn a_leading_home_is_taken_off_the_arguments() {
        let args = strings(&["--home", "data", "item", "list"]);
        let (home, rest) = split_home(&args);
        assert_eq!(home, Some(PathBuf::from("data")));
        assert_eq!(rest, &args[2..]);
        let args = strings(&["--home=data", "version"]);
        let (home, rest) = split_home(&args);
        assert_eq!(home, Some(PathBuf::from("data")));
        assert_eq!(rest, &args[1..]);
        let args = strings(&["echo", "--home", "data"]);
        assert_eq!(split_home(&args), (None, args.as_slice()));
    }
}
