//! Faults a test can switch on, so that a failure is reproduced on demand.
//!
//! The harness writes them to a file in the control folder. The program reads
//! that file at every start and uses up the faults that apply to it.

use serde::{Deserialize, Serialize};

/// Where in a command the program stops, for [`FaultKind::Stop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopPoint {
    /// Before the command does anything.
    Start,
    /// In a write: the new contents are in the temporary file and the lock is
    /// held, but nothing is committed. A read never reaches this point.
    BeforeCommit,
    /// In a write: the commit is done and the lock released, but nothing has
    /// been printed. A read never reaches this point.
    AfterCommit,
}

/// How the program stops, for [`FaultKind::Stop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopHow {
    /// Wait until killed. The control interface can tell when the program is
    /// waiting. After five minutes it gives up and exits with status 70.
    Hang,
    /// Exit at once with this status and print nothing more.
    Exit {
        /// The exit status.
        code: u8,
    },
    /// End abnormally: by a signal where the system has signals, and with a
    /// status that means a crash elsewhere.
    Abort,
}

/// One of the program's two output streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// What a fault makes the program do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaultKind {
    /// Answer correctly, after waiting this long.
    Delay {
        /// Milliseconds to wait before doing anything.
        ms: u64,
    },
    /// Stop at a point in the command: hang, exit, or end abnormally.
    Stop {
        /// Where to stop.
        at: StopPoint,
        /// How to stop.
        how: StopHow,
    },
    /// Print every output form differently: other column names, another
    /// column, other wording. The version stays the same.
    Drift,
    /// Ask a question on standard output and wait for an answer on standard
    /// input before doing anything, whatever the command and its flags.
    Prompt {
        /// The question, printed with no line end after it.
        text: String,
    },
    /// Print this many bytes of filler lines on a stream before anything else.
    Flood {
        /// The stream to fill.
        stream: Stream,
        /// At least this many bytes are printed.
        bytes: u64,
    },
    /// Print this text on a stream, exactly as given, before anything else.
    /// A test uses it to put a registered secret at a chosen place in what
    /// the program prints.
    Say {
        /// The stream to print on.
        stream: Stream,
        /// The text, with no line end added.
        text: String,
    },
    /// Store a quantity rounded down to a multiple, while printing the
    /// quantity that was asked for.
    RoundQuantity {
        /// The multiple to round down to.
        multiple: u32,
    },
    /// Store a new item's name in upper case, while printing it as given.
    UppercaseName,
    /// Report this version. Unless a command is named it applies to the
    /// version command.
    ReportVersion {
        /// The version to report, such as `9.0.0`.
        version: String,
    },
    /// Refuse every command that reads or writes data, with the status for an
    /// expired session, unless the program is signed in.
    RequireSession,
    /// Print the secrets the program was given: the password `login` read, and
    /// the token variable. On both streams.
    LeakSecret,
    /// Set out every listing of commands in help with a tab between a name
    /// and its text, as programs written in one widely used language do.
    /// It applies to `help`, which a fault that names no command skips.
    TabbedHelp,
    /// Add to the program's listing of commands in help one line that holds
    /// no command name a reader can take: `  [options]` and a text. It
    /// applies to `help`.
    UnnamedHelpLine,
    /// Answer `item show` of a code the store does not hold with no output
    /// and status 0, as a program that answers "none" with silence does.
    SilentMiss,
    /// Print a second line after the version line, the version of the
    /// program's interface. It applies to the version command only.
    InterfaceLine,
    /// Print `item list --format=json` as one nested document: a header,
    /// and the items under the key `items`.
    NestedJson,
    /// Start a child process that keeps the program's output streams and
    /// outlives it.
    SpawnChild {
        /// The child ends by itself after this long, in milliseconds.
        lifetime_ms: u64,
    },
}

impl FaultKind {
    /// The prompt the program itself uses before a write: `Are you sure? [y/N] `.
    #[must_use]
    pub fn prompt() -> Self {
        Self::Prompt {
            text: crate::CONFIRM_PROMPT.to_owned(),
        }
    }

    /// A stop at a point, in a way.
    #[must_use]
    pub fn stop(at: StopPoint, how: StopHow) -> Self {
        Self::Stop { at, how }
    }
}

/// One fault and the starts of the program it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// What the program does.
    #[serde(flatten)]
    pub kind: FaultKind,
    /// Only starts of this command are affected: `item set`, or `item` for
    /// every item command. See [`Fault::applies_to`] for the rule when absent.
    #[serde(default)]
    pub command: Option<String>,
    /// How many starts it applies to before it clears itself. Until cleared, if absent.
    #[serde(default)]
    pub times: Option<u32>,
}

impl Fault {
    /// A fault that affects the next start it applies to, and no other.
    #[must_use]
    pub fn once(kind: FaultKind) -> Self {
        Self {
            kind,
            command: None,
            times: Some(1),
        }
    }

    /// A fault that stays on until the faults are cleared.
    #[must_use]
    pub fn until_cleared(kind: FaultKind) -> Self {
        Self {
            kind,
            command: None,
            times: None,
        }
    }

    /// The same fault, limited to one command or one group of commands.
    #[must_use]
    pub fn on(mut self, command: &str) -> Self {
        self.command = Some(command.to_owned());
        self
    }

    /// Whether this fault applies to a start of `command`, a name such as
    /// `item set`, `version`, or `help`.
    ///
    /// A fault that names a command applies to that command, and to every
    /// command of the group if it names a group. A fault that names none
    /// applies to every command except `help` and `version`, so that a fault
    /// meant for a command is not used up by a version check before it; a
    /// [`FaultKind::ReportVersion`] that names none applies to `version` only.
    #[must_use]
    pub fn applies_to(&self, command: &str) -> bool {
        match &self.command {
            Some(named) => {
                command == named
                    || command
                        .strip_prefix(named.as_str())
                        .is_some_and(|rest| rest.starts_with(' '))
            }
            None if matches!(
                self.kind,
                FaultKind::ReportVersion { .. } | FaultKind::InterfaceLine
            ) =>
            {
                command == "version"
            }
            None => command != "help" && command != "version",
        }
    }
}

/// The contents of the faults file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FaultFile {
    pub(crate) faults: Vec<Fault>,
}

impl FaultFile {
    /// The faults that apply to a start of `command`, each used up by one.
    pub(crate) fn take_for(&mut self, command: &str) -> Vec<FaultKind> {
        let mut taken = Vec::new();
        self.faults.retain_mut(|fault| {
            if !fault.applies_to(command) {
                return true;
            }
            taken.push(fault.kind.clone());
            match fault.times.as_mut() {
                Some(times) => {
                    *times = times.saturating_sub(1);
                    *times > 0
                }
                None => true,
            }
        });
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_with_no_command_skips_help_and_version() {
        let fault = Fault::once(FaultKind::Drift);
        assert!(fault.applies_to("item set"));
        assert!(fault.applies_to("echo"));
        assert!(!fault.applies_to("help"));
        assert!(!fault.applies_to("version"));
        let version = Fault::once(FaultKind::ReportVersion {
            version: "9.0.0".to_owned(),
        });
        assert!(version.applies_to("version"));
        assert!(!version.applies_to("item show"));
    }

    #[test]
    fn a_named_command_matches_itself_and_its_group_only() {
        let fault = Fault::once(FaultKind::Drift).on("item");
        assert!(fault.applies_to("item"));
        assert!(fault.applies_to("item list"));
        assert!(!fault.applies_to("items"));
        assert!(Fault::once(FaultKind::Drift).on("help").applies_to("help"));
    }

    #[test]
    fn a_fault_is_used_up_by_the_starts_it_applies_to() {
        let mut file = FaultFile {
            faults: vec![
                Fault::once(FaultKind::Drift),
                Fault::until_cleared(FaultKind::UppercaseName).on("item add"),
            ],
        };
        assert!(file.take_for("version").is_empty());
        assert_eq!(file.take_for("item show"), vec![FaultKind::Drift]);
        assert!(file.take_for("item show").is_empty());
        assert_eq!(file.take_for("item add"), vec![FaultKind::UppercaseName]);
        assert_eq!(file.faults.len(), 1);
    }

    #[test]
    fn a_fault_survives_the_faults_file() -> Result<(), serde_json::Error> {
        let faults = vec![
            Fault::once(FaultKind::stop(
                StopPoint::AfterCommit,
                StopHow::Exit { code: 75 },
            )),
            Fault::until_cleared(FaultKind::Flood {
                stream: Stream::Stderr,
                bytes: 10,
            })
            .on("item"),
        ];
        let file = FaultFile { faults };
        let text = serde_json::to_string(&file)?;
        assert_eq!(serde_json::from_str::<FaultFile>(&text)?, file);
        Ok(())
    }
}
