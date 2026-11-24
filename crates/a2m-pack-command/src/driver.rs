//! The `cli` driver: prepare checks the program, `spawn` starts it once
//! through the supervisor and waits for it to end, `expect` reads how it
//! ended, and `parse` reads what it printed.
//!
//! The driver never retries, never builds a command line, and gives a
//! program nothing its `commands/` file does not list. A write whose result
//! nothing confirms is `possible_commit`, whatever went wrong.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_auth::SessionLease;
use a2m_core::{
    CodeEntry, Digest, EnvironmentKind, Failure, RecordKey, Redact, Step, Value, Values,
};
use a2m_provider::{
    Access, BoxFuture, Capture, CheckedPath, Cleanup, ConnFailure, Deadline, Driver, DriverHealth,
    EntryKind, EnvAllowlist, Exit, Finished, LaunchSpec, Leftover, Observed, OneShot, Posture,
    PrepareContext, Prepared, ProgramRef, Ran, RootName, Roots, ScopedSupervisor, StepFailure,
    StepInput, StepOutcome, SubsystemKind, SurfaceRef, current_platform,
};
use a2m_telemetry::{Acting, Detail};

use crate::args::{self, Given};
use crate::codes::{
    self, FILE_KIND, PACK, SURFACE_KIND, answered, not_sent, not_usable, possible_commit, shown,
};
use crate::model::{
    Command, Effect, ExitMeaning, Limits, Probe, Program, Script, Section, StepPayload,
    VersionCheck,
};
use crate::output::{self, Parsed};
use crate::pattern;
use crate::safety::{self, Refused, Role, Trust};

/// The root the pack makes one fresh folder in for each run.
pub(crate) const RUNS_ROOT: &str = "command_runs";
/// The kind of subsystem a started program is counted as.
pub(crate) const SUBSYSTEM: &str = "command";
/// How long a help or version form may run.
const PROBE_LIMIT_MS: u32 = 10_000;
/// How many names are tried for a run's folder before the run is refused.
const FOLDER_TRIES: u32 = 64;

/// The driver of the `cli` mode.
pub(crate) struct CliDriver {
    pub roots: Roots,
    pub redact: Arc<dyn Redact>,
    pub runs_root: RootName,
    pub kind: SubsystemKind,
    pub max_timeout_ms: u64,
    pub max_output_bytes: u64,
    pub next_folder: AtomicU64,
    /// The groups whose write counts as the administrator's.
    pub trust: Trust,
}

/// How the last start of a program ended, kept between its three steps.
struct Last {
    command: String,
    /// Whether the command writes: a step that cannot read how such a start
    /// ended leaves a write in doubt.
    writes: bool,
    finished: Finished,
    /// What `expect` made of the exit status: `Some(true)` for `ok`,
    /// `Some(false)` for `absent`.
    done: Option<bool>,
}

/// What `prepare` acquired for one run on one surface.
struct Session {
    section: Section,
    surface: String,
    path: PathBuf,
    digest: Digest,
    supervisor: ScopedSupervisor,
    posture: Posture,
    test_environment: bool,
    workdir: CheckedPath,
    /// Where the copies of the target's scripts are written, when the
    /// surface's program is the interpreter of fixed pairs.
    scripts: Option<CheckedPath>,
    /// The project folder of the request that started the run, in which the
    /// target's folder is `targets/<target>`.
    project: Option<CheckedPath>,
    /// The target's name.
    target: String,
    last: Option<Last>,
}

fn joined<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = Option<T>> {
    let task = tokio::task::spawn_blocking(work);
    async move { task.await.ok() }
}

fn refused(why: &Refused, path: &Path) -> StepFailure {
    let at = shown(&path.display().to_string());
    match why {
        Refused::Missing => not_sent(
            &codes::row::PROGRAM_MISSING,
            format!("The program `{at}` is not there, is not a file, or cannot be run."),
            "Install the program at the path its surface declares, or correct the path in `target.yaml`.",
        ),
        Refused::NotAbsolute => not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!("The program `{at}` is not named by an absolute path."),
            "Name the program by its absolute path in `target.yaml`; it is never searched for.",
        ),
        Refused::Script => not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!(
                "The program `{at}` is a shell, an interpreter, a program that runs code its arguments name, or a script that one of them would run."
            ),
            "Name a program that takes its arguments as data; a shell or a script is never started.",
        ),
        Refused::Writable(what) => not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!(
                "The program `{at}` could be replaced by another user: `{}` is theirs to write.",
                shown(&what.display().to_string())
            ),
            "Give the file and every folder above it to the engine's user or the administrator, with no write access for anyone else.",
        ),
        Refused::Unreadable(what) => not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!(
                "The program `{at}` is not started: who may change `{}` could not be read.",
                shown(&what.display().to_string())
            ),
            "Give the file and every folder above it an access list the engine's user can read, naming only that user and the system's own identities.",
        ),
        Refused::Link(what) => not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!(
                "The program `{at}` is not started: `{}` is a link, which is not followed on this system.",
                shown(&what.display().to_string())
            ),
            "Name the program by the path it has with no link on the way.",
        ),
        Refused::Elsewhere => not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!(
                "The program `{at}` is on another machine's share, a device, a drive that is not fixed in this machine, or a drive letter that stands for a folder."
            ),
            "Install the program on a local drive and name it by that path.",
        ),
    }
}

fn digest_changed(path: &Path, why: &str) -> StepFailure {
    not_sent(
        &codes::row::PROGRAM_DIGEST_CHANGED,
        format!(
            "The program `{}` {why}.",
            shown(&path.display().to_string())
        ),
        "Review the program that is installed now, add its SHA-256 under `program.digests` for this platform, and publish.",
    )
}

/// The failure of a step that reads how a start ended and cannot be done
/// as written. When the last start was of a command that writes, nothing
/// confirms that write, and the failure says so instead.
fn unconfirmed(session: &Session, failure: StepFailure) -> StepFailure {
    if session.last.as_ref().is_some_and(|last| last.writes) {
        possible_commit("the step that confirms it cannot be done as the plan wrote it")
    } else {
        failure
    }
}

/// A program whose process group could not be ended has not finished:
/// what it started may still be running, and still writing.
fn left_running(finished: &Finished, writes: bool) -> Option<StepFailure> {
    if finished.cleanup.complete {
        return None;
    }
    Some(if writes {
        possible_commit("left something running that could not be ended")
    } else {
        answered(
            &a2m_provider::codes::row::CLEANUP_INCOMPLETE,
            "The program ended, and something it started could not be ended.",
            "End what the program left running; new starts on the surface wait until then.",
        )
    })
}

/// Whether the supervisor kept less of standard output than was written.
fn stdout_cut(finished: &Finished) -> bool {
    finished.stdout_bytes > finished.stdout.len() as u64
}

impl CliDriver {
    fn text(&self, bytes: &[u8]) -> String {
        self.redact.redact(&String::from_utf8_lossy(bytes))
    }

    /// What a program said on standard error, as a failure may quote it.
    ///
    /// The whole text is redacted first. Control characters are then taken
    /// out, which can join the two halves of a secret that one of them
    /// divided, so the text is redacted again. The cut comes last: a cut
    /// before the redactor would leave part of a secret that no redactor
    /// can match.
    fn said(&self, stderr: &str) -> String {
        let whole = self.redact.redact(stderr.trim());
        let clean: String = whole.chars().filter(|c| !c.is_control()).collect();
        shown(&self.redact.redact(&clean))
    }

    fn capture(&self, name: &str, bytes: &[u8]) -> Capture {
        Capture {
            name: name.to_owned(),
            media_type: "text/plain; charset=utf-8".to_owned(),
            bytes: self.text(bytes).into_bytes(),
        }
    }

    /// Starts the session's program once and waits for it to end.
    async fn start(
        &self,
        session: &Session,
        elements: Vec<String>,
        program: Option<&Program>,
        limits: Limits,
        deadline: Deadline,
        writes: bool,
    ) -> Result<Finished, StepFailure> {
        let allowed_ms = u64::from(limits.timeout_ms)
            .min(self.max_timeout_ms)
            .min(u64::try_from(deadline.remaining().as_millis()).unwrap_or(u64::MAX));
        if allowed_ms == 0 {
            return Err(not_sent(
                &a2m_provider::codes::row::TIMEOUT,
                "No time was left to start the program.",
                "Try again.",
            ));
        }
        // The engine's own variables are never handed to a program, whatever
        // a plan says: the compiler refuses them, and so does the driver.
        if let Some(name) = program.and_then(|program| {
            program
                .env_set
                .keys()
                .chain(program.env_pass.iter())
                .find(|name| a2m_provider::is_engine_variable(name))
        }) {
            return Err(not_sent(
                &codes::row::STEP_NOT_USABLE,
                format!(
                    "The plan hands the program the variable `{}`, which belongs to the engine.",
                    name.chars().take(64).collect::<String>()
                ),
                "Compile the target again from files that do not name the engine's variables.",
            ));
        }
        // Nor a variable that makes a program load code other than its own:
        // the compiler refuses them, the driver, and the supervisor.
        if let Some(name) = program.and_then(|program| {
            program
                .env_set
                .keys()
                .chain(program.env_pass.iter())
                .find(|name| a2m_provider::redirects_loading(name))
        }) {
            return Err(not_sent(
                &codes::row::STEP_NOT_USABLE,
                format!(
                    "The plan hands the program the variable `{}`, which makes a program load code other than its own.",
                    name.chars().take(64).collect::<String>()
                ),
                "Compile the target again from files that do not name such a variable.",
            ));
        }
        let pass = passed(program);
        let set: Vec<(String, OsString)> = program
            .map(|program| {
                program
                    .env_set
                    .iter()
                    .map(|(name, value)| (name.clone(), OsString::from(value)))
                    .collect()
            })
            .unwrap_or_default();
        let spec = LaunchSpec {
            program: ProgramRef {
                path: session.path.clone(),
                digest: Some(session.digest),
            },
            args: elements.into_iter().map(OsString::from).collect(),
            env: EnvAllowlist { pass, set },
            cwd: Some(session.workdir.clone()),
            kind: self.kind.clone(),
            // One start at a time on a surface.
            place: Some(session.path.display().to_string()),
        };
        let link = OneShot {
            max_output_bytes: limits
                .stdout_bytes
                .max(limits.stderr_bytes)
                .min(self.max_output_bytes),
            time_limit: Duration::from_millis(allowed_ms),
            stdin: None,
        };
        match session.supervisor.run(spec, link).await {
            Ok(finished) => match left_running(&finished, writes) {
                Some(failure) => Err(failure),
                None => Ok(finished),
            },
            Err(ConnFailure {
                failure,
                ran: Ran::Unknown,
            }) => Err(StepFailure::unknown(failure)),
            Err(ConnFailure {
                failure,
                ran: Ran::NotStarted,
            }) => Err(self.not_started(session, failure).await),
        }
    }

    /// Names, in the pack's own codes, why the supervisor started nothing.
    async fn not_started(&self, session: &Session, failure: Failure) -> StepFailure {
        use a2m_provider::codes as contract;
        let path = session.path.clone();
        if failure.code() == &contract::PROGRAM_NOT_FOUND {
            return refused(&Refused::Missing, &path);
        }
        if failure.code() == &contract::SUBSYSTEM_LIMIT {
            return not_sent(
                &codes::row::PROGRAM_BUSY,
                "Another start of the program holds the surface.",
                "Try again when it has ended.",
            );
        }
        if failure.code() == &contract::EFFECT_NOT_DECLARED {
            let read = path.clone();
            let now = joined(move || safety::digest_of(&read).ok())
                .await
                .flatten();
            if now != Some(session.digest) {
                return digest_changed(
                    &path,
                    "changed after it was checked: its digest is another now",
                );
            }
        }
        StepFailure::not_sent(failure)
    }

    /// Runs a program's version command and checks what it reports.
    async fn version(
        &self,
        session: &Session,
        check: &VersionCheck,
        deadline: Deadline,
    ) -> Result<String, StepFailure> {
        let limits = Limits {
            timeout_ms: PROBE_LIMIT_MS,
            ..Limits::default()
        };
        let finished = self
            .start(session, check.args.clone(), None, limits, deadline, false)
            .await?;
        let unsupported = |why: &str| {
            answered(
                &codes::row::PROGRAM_VERSION_UNSUPPORTED,
                format!(
                    "The program `{}` {why}.",
                    shown(&session.path.display().to_string())
                ),
                "Install a supported version, or check the target's files against this one and add it to `program.version.supported`.",
            )
        };
        match reported_version(check, &finished, &self.text(&finished.stdout)) {
            Some(version) if check.supported.contains(&version) => Ok(version),
            Some(_) => Err(unsupported(
                "reports a version the target's files were not written for",
            )),
            None => Err(unsupported(
                "did not report its version in the declared form",
            )),
        }
    }

    async fn prepare_session(&self, cx: &PrepareContext<'_>) -> Result<Session, StepFailure> {
        let surface = cx.surface.surface.name.as_str().to_owned();
        let path = PathBuf::from(&cx.surface.surface.address);
        let section = cx
            .plan
            .packs
            .iter()
            .find(|section| {
                section.pack == PACK
                    && section.kind == FILE_KIND
                    && section.surface_kind == SURFACE_KIND
            })
            .map(|section| section.payload.decode::<Section>())
            .transpose()
            .map_err(StepFailure::not_sent)?
            .unwrap_or_default();

        // The file itself: there, a program, and nobody else's to replace.
        // A program is the interpreter of fixed pairs only when every file
        // on its surface says so; then it is not refused by its name.
        let on_surface: Vec<&Program> = section
            .programs
            .iter()
            .filter(|program| program.surface == surface)
            .collect();
        let interprets = !on_surface.is_empty()
            && on_surface
                .iter()
                .all(|program| program.interpreter.is_some());
        // A program of the provider's closed list of programs whose listed
        // commands run no code but their own is started only in those
        // forms: every argument list its files give it, by the row of its
        // declared name and by the row of the file its path leads to, is
        // one of them (decision 0299, step one). The plan is not taken at
        // its word.
        let own_code =
            !interprets && a2m_provider::runs_its_own_code(&path.to_string_lossy()).is_some();
        if own_code {
            let real = std::fs::canonicalize(&path).ok();
            for name in std::iter::once(path.clone()).chain(real) {
                let shown_name = shown(&name.display().to_string());
                let Some(row) = a2m_provider::runs_its_own_code(&name.to_string_lossy()) else {
                    return Err(refused(&Refused::Script, &name));
                };
                for (index, program) in section.programs.iter().enumerate() {
                    if program.surface != surface {
                        continue;
                    }
                    let commands = section
                        .commands
                        .iter()
                        .filter(|(_, command)| command.program as usize == index)
                        .map(|(name, command)| (name.as_str(), command));
                    if let Some(why) = crate::own_code::refusal(row, &shown_name, program, commands)
                    {
                        return Err(not_sent(
                            &codes::row::PROGRAM_PATH_UNSAFE,
                            why,
                            "Give the program only the commands of it that run no code but its own, in their exact form, and publish.",
                        ));
                    }
                }
            }
        }
        let role = if interprets {
            Role::Interpreter
        } else if own_code {
            Role::OwnCode
        } else {
            Role::Program
        };
        // An interpreter is started only in the one form the provider's
        // closed table gives it, by its declared name and by the name of
        // the file its path leads to (decision 0250).
        if interprets {
            let real = std::fs::canonicalize(&path).ok();
            for program in &on_surface {
                let fixed = program.interpreter.clone().unwrap_or_default();
                let names = std::iter::once(path.clone()).chain(real.clone());
                for name in names {
                    if a2m_provider::interpreter_form(&name.to_string_lossy(), &fixed).is_err() {
                        return Err(not_sent(
                            &codes::row::PROGRAM_PATH_UNSAFE,
                            format!(
                                "The interpreter `{}` is not started with the fixed arguments its files declare: they are not the one form in which it reads a script file and takes what follows as data.",
                                shown(&name.display().to_string())
                            ),
                            "Declare an interpreter the pack starts as a pair, with the fixed arguments of its form, and publish.",
                        ));
                    }
                }
            }
        }
        let checked = path.clone();
        let trust = self.trust.clone();
        let verdict = joined(move || {
            safety::check(&checked, &trust, role).map(|()| safety::digest_of(&checked).ok())
        })
        .await;
        let digest = match verdict {
            Some(Ok(Some(digest))) => digest,
            Some(Err(why)) => return Err(refused(&why, &path)),
            Some(Ok(None)) | None => return Err(refused(&Refused::Missing, &path)),
        };
        let platform = current_platform();
        let listed = section
            .programs
            .iter()
            .filter(|program| program.surface == surface)
            .filter_map(|program| program.digests.get(&platform))
            .flatten()
            .any(|known| *known == digest.to_string());
        if !listed {
            return Err(digest_changed(
                &path,
                &format!("is not a build the target's files list for `{platform}`"),
            ));
        }

        let workdir = self.fresh_folder(cx)?;
        let scripts = if interprets {
            match self.fresh_folder(cx) {
                Ok(folder) => Some(folder),
                Err(failure) => {
                    remove_folder(&workdir);
                    return Err(failure);
                }
            }
        } else {
            None
        };

        let test_environment = cx
            .plan
            .environment_named(&cx.surface.environment)
            .is_some_and(|environment| environment.kind == EnvironmentKind::Test);
        let session = Session {
            section,
            surface,
            path,
            digest,
            supervisor: cx.supervisor.clone(),
            posture: cx.posture,
            test_environment,
            workdir,
            scripts,
            project: cx.paths.project().cloned(),
            target: cx.surface.target.as_str().to_owned(),
            last: None,
        };
        // The version, once for this run, for each file that declares one.
        let checks: Vec<VersionCheck> = session
            .section
            .programs
            .iter()
            .filter(|program| program.surface == session.surface)
            .filter_map(|program| program.version.clone())
            .collect();
        for check in &checks {
            if let Err(failure) = self.version(&session, check, cx.deadline).await {
                remove_folder(&session.workdir);
                if let Some(scripts) = &session.scripts {
                    remove_folder(scripts);
                }
                return Err(failure);
            }
        }
        Ok(session)
    }

    /// Makes the run's own folder: one that was not there before, and
    /// that holds nothing.
    ///
    /// The name is the run, this process, and a count that starts again
    /// with each engine, so an earlier engine with the same process number
    /// may have left a folder of the same name. Such a folder is never
    /// used, emptied, or removed: the next name is taken.
    fn fresh_folder(&self, cx: &PrepareContext<'_>) -> Result<CheckedPath, StepFailure> {
        let run: String = cx
            .run
            .to_string()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let root = self
            .roots
            .folder(&self.runs_root, Access::Write)
            .map_err(StepFailure::not_sent)?;
        // Each folder this makes is the engine's user's alone from the
        // moment it exists; one that was there already is checked where a
        // script is put in it.
        root.create_private_dir_all()
            .map_err(StepFailure::not_sent)?;
        for _ in 0..FOLDER_TRIES {
            let number = self.next_folder.fetch_add(1, Ordering::Relaxed);
            let folder = root
                .join(&format!("{run}-{}-{number}", std::process::id()))
                .map_err(StepFailure::not_sent)?;
            // Made new, or not at all: a folder this call makes was empty
            // and the run's own when it was made. Whatever already stands
            // at the name is left as it is, and the next name is taken.
            match folder.create_private_dir() {
                Ok(()) => return Ok(folder),
                Err(failure) => {
                    if matches!(folder.entry_kind(), Ok(EntryKind::Absent) | Err(_)) {
                        return Err(StepFailure::not_sent(failure));
                    }
                }
            }
        }
        Err(StepFailure::not_sent(
            a2m_provider::codes::row::FILE_UNAVAILABLE.failure(
                "No empty folder could be made for the run: every name tried was already taken.",
                "Remove what earlier runs left under the command pack's `command_runs` root.",
            ),
        ))
    }

    async fn probe(
        &self,
        session: &Session,
        program: u32,
        probe: &Probe,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let absent = Ok(StepOutcome::Absent(Observed::default()));
        let Some(program) = session
            .section
            .programs
            .get(program as usize)
            .filter(|program| program.surface == session.surface)
        else {
            return absent;
        };
        let probed = match probe {
            Probe::Version => "version",
            Probe::Help { .. } => "help",
        };
        let elements = match probe {
            Probe::Version => match &program.version {
                Some(check) => check.args.clone(),
                None => return absent,
            },
            Probe::Help { index, command } => {
                let Some(form) = program.help.get(*index as usize) else {
                    return absent;
                };
                let takes_command = form.iter().any(|element| element == "{command}");
                if takes_command != command.is_some() {
                    return absent;
                }
                let words: Vec<&str> = command
                    .as_deref()
                    .map(|command| command.split(' ').collect())
                    .unwrap_or_default();
                let plain = |word: &&str| {
                    word.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                        && word
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
                };
                if !words.iter().all(plain) {
                    return Err(not_sent(
                        &codes::row::ARGUMENT_UNSAFE,
                        "A command's name is words of letters, digits, and `_.:-`, and this one is not.",
                        "Ask for the help of a command the program's own help lists.",
                    ));
                }
                form.iter()
                    .flat_map(|element| {
                        if element == "{command}" {
                            words.iter().map(|word| (*word).to_owned()).collect()
                        } else {
                            vec![element.clone()]
                        }
                    })
                    .collect()
            }
        };
        let limits = Limits {
            timeout_ms: PROBE_LIMIT_MS,
            ..Limits::default()
        };
        let shown_elements = elements.clone();
        // A bare environment: nothing of the file's `env` is given.
        let finished = self
            .start(session, elements, None, limits, input.deadline, false)
            .await?;
        // Output the supervisor cut is not shown at all: the cut was made
        // before the redactor could see the text, and may have divided a
        // secret. `truncated` says that it happened.
        let stdout = if stdout_cut(&finished) {
            String::new()
        } else {
            self.text(&finished.stdout)
        };
        let version = match (probe, &program.version) {
            (Probe::Version, Some(check)) => {
                reported_version(check, &finished, &stdout).map(|version| (check, version))
            }
            _ => None,
        };
        let mut values = Values::new()
            .with("stdout", stdout)
            .with(
                "elements",
                shown_elements
                    .into_iter()
                    .map(Value::from)
                    .collect::<Vec<Value>>(),
            )
            .with("digest", session.digest.to_string())
            .with("truncated", finished.truncated());
        if let Exit::Code(code) = finished.exit {
            values.insert("exit", i64::from(code));
        }
        if let Some((check, version)) = version {
            values.insert("pattern", check.stdout.clone());
            // Read from text that was redacted whole.
            values.insert("version", version);
        }
        let acting = Acting {
            means: vec![
                Detail::new("probe", probed),
                Detail::new("exit", exit_word(&finished.exit)),
            ],
            measured: vec![Detail::new(
                "stdout_bytes",
                finished.stdout_bytes.to_string(),
            )],
        };
        Ok(StepOutcome::Done(Observed {
            values,
            captures: vec![Capture::acting(&acting)],
            ..Observed::default()
        }))
    }

    async fn spawn(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        // Whatever this start comes to, no later step reads an earlier one.
        session.last = None;
        let payload: StepPayload = step.payload.decode().map_err(StepFailure::not_sent)?;
        let key_params = match payload {
            StepPayload::Probe { program, probe } => {
                return self.probe(session, program, &probe, input).await;
            }
            StepPayload::Command { key_params, .. } => key_params,
        };
        // In the sandbox a command runs only against a test environment.
        // There the guard is lifted for one step only: the step marked
        // `commits`, in the context the engine prepared to write a declared
        // test record. Every other step there, and a step marked `commits`
        // in any other sandbox context, keeps the guard: the engine cannot
        // see what such a step does.
        let allowed = match session.posture {
            Posture::Live => true,
            Posture::Sandbox { test_records } => {
                session.test_environment && (!step.commits || test_records)
            }
            Posture::Discovery => false,
        };
        if !allowed {
            return Err(not_sent(
                &a2m_provider::codes::row::READ_ONLY_POSTURE,
                "A command of a program was asked for where only its declared help and version forms may run: nothing can make a program unable to write.",
                "Try the command against an environment of kind `test`, or run the operation live.",
            ));
        }
        let (name, command, program) = found(session, step)?;
        let name = name.to_owned();
        // A command that writes is started only by the step the plan marks
        // as a commit point, for a record the ledger holds an intent for.
        // The handle lets such a step through only with the ledger's token;
        // the compiler's word that the plan is so is not relied on here.
        let writes = command.effect != Effect::Read;
        if writes && !step.commits {
            return Err(not_usable(&format!(
                "the command `{name}` writes, and the step is not marked as a commit point"
            )));
        }
        if !writes && step.commits {
            return Err(not_usable(&format!(
                "the step is marked as a commit point, and the command `{name}` is declared `read`"
            )));
        }
        if writes && input.record.is_none() {
            return Err(not_usable(&format!(
                "the command `{name}` writes, and the step belongs to no record of the ledger"
            )));
        }
        let elements = args::elements(
            command,
            &Given {
                values: input.args,
                key_params: &key_params,
                key: input.key,
                stamp: input.stamp,
            },
        )?;
        // A fixed pair: the interpreter's own arguments, the path of the
        // checked copy of the script, then the typed values, each its own
        // element after the script's path, where the interpreter reads none
        // of them.
        let elements = match (&program.interpreter, &command.script) {
            (None, None) => elements,
            (Some(fixed), Some(script)) => {
                let copy = script_copy(session, &name, script, &self.trust)?;
                let mut whole = fixed.clone();
                whole.push(copy);
                whole.extend(elements);
                whole
            }
            (Some(_), None) => {
                return Err(not_usable(&format!(
                    "the command `{name}` names no script, and its program is an interpreter"
                )));
            }
            (None, Some(_)) => {
                return Err(not_usable(&format!(
                    "the command `{name}` names a script, and its program is no interpreter"
                )));
            }
        };
        let limits = command.limits;
        let finished = self
            .start(
                session,
                elements,
                Some(program),
                limits,
                input.deadline,
                writes,
            )
            .await?;
        let over = [
            ("stdout", finished.stdout_bytes, limits.stdout_bytes),
            ("stderr", finished.stderr_bytes, limits.stderr_bytes),
        ]
        .into_iter()
        .find(|(_, wrote, ceiling)| *wrote > (*ceiling).min(self.max_output_bytes));
        if let Some((stream, _, ceiling)) = over {
            return Err(if writes {
                possible_commit("printed more than its ceiling allows")
            } else {
                answered(
                    &codes::row::LIMIT_EXCEEDED,
                    format!(
                        "The command `{name}` printed more than its ceiling `limits.{stream}` of {ceiling} bytes."
                    ),
                    "Narrow what the command lists, or raise `limits` in its `commands/` file and the setting `packs.command.max_output_bytes`.",
                )
            });
        }
        let captures = vec![
            Capture::acting(&Acting {
                means: vec![
                    Detail::new("command", &name),
                    Detail::new("exit", exit_word(&finished.exit)),
                ],
                measured: vec![
                    Detail::new("stdout_bytes", finished.stdout_bytes.to_string()),
                    Detail::new("stderr_bytes", finished.stderr_bytes.to_string()),
                ],
            }),
            self.capture("stdout", &finished.stdout),
            self.capture("stderr", &finished.stderr),
        ];
        session.last = Some(Last {
            command: name,
            writes,
            finished,
            done: None,
        });
        Ok(StepOutcome::Done(Observed {
            captures,
            ..Observed::default()
        }))
    }

    fn expect(&self, session: &mut Session, step: &Step) -> Result<StepOutcome, StepFailure> {
        let (name, command, program) =
            found(session, step).map_err(|failure| unconfirmed(session, failure))?;
        let Some(last) = session.last.as_ref().filter(|last| last.command == name) else {
            return Err(unconfirmed(
                session,
                not_usable("its program was not started first"),
            ));
        };
        let writes = command.effect != Effect::Read;
        let stdout = String::from_utf8_lossy(&last.finished.stdout);
        let stderr = String::from_utf8_lossy(&last.finished.stderr);
        let asked = program.prompts.iter().any(|prompt| {
            pattern::prompt(prompt)
                .is_ok_and(|compiled| compiled.is_match(&stdout) || compiled.is_match(&stderr))
        });
        if asked {
            return Err(if writes {
                possible_commit("asked a question nobody was there to answer")
            } else {
                answered(
                    &codes::row::UNEXPECTED_PROMPT,
                    format!(
                        "The command `{name}` asked a question, and a program is never given a person to answer one."
                    ),
                    "Add the argument that makes the command not ask, such as `--yes`, to its `args` in `commands/`.",
                )
            });
        }
        let meaning = match last.finished.exit {
            Exit::Code(code) => command.exits.get(&code.to_string()).copied(),
            Exit::Killed => None,
        };
        // What the program said on standard error: redacted, cleaned, and
        // only then cut. It is untrusted text, carried apart from the
        // failure's own sentence.
        let said = self.said(&stderr);
        let own = |row: &CodeEntry, what: &str, next: &str| {
            answered(row, format!("The command `{name}` {what}."), next).with_said(&said)
        };
        let done = match meaning {
            Some(ExitMeaning::Ok) => true,
            Some(ExitMeaning::Absent) if step.lookup => false,
            Some(ExitMeaning::Absent) => {
                return Err(own(
                    &codes::row::RECORD_ABSENT,
                    "found no record of that name, and changed nothing",
                    "Read the record again; it may have been removed.",
                ));
            }
            Some(ExitMeaning::Input) => {
                return Err(own(
                    &codes::row::SCHEMA_INVALID,
                    "refused its arguments as badly formed, and changed nothing",
                    "Correct the values and call again.",
                ));
            }
            Some(ExitMeaning::Rejected) => {
                return Err(own(
                    &codes::row::APP_VALIDATION_ERROR,
                    "refused the values by a rule of its own, and changed nothing",
                    "Correct the values and call again.",
                ));
            }
            Some(ExitMeaning::Stale) => {
                return Err(own(
                    &codes::row::VERSION_CONFLICT,
                    "found the record changed by someone else, and changed nothing",
                    "Read the record again and decide whether the change still applies.",
                ));
            }
            Some(ExitMeaning::Busy) => {
                return Err(own(
                    &codes::row::PROGRAM_BUSY,
                    "found what it needs held by another, and changed nothing",
                    "Try again shortly.",
                ));
            }
            Some(ExitMeaning::Transient) => {
                return Err(own(
                    &codes::row::UPSTREAM_UNAVAILABLE,
                    "asked to be tried again later, and changed nothing",
                    "Try again shortly.",
                ));
            }
            Some(ExitMeaning::SessionExpired) => {
                return Err(own(
                    &a2m_auth::codes::row::NEEDS_LOGIN,
                    "is not signed in, and changed nothing",
                    "Sign the program in again.",
                ));
            }
            None if writes => {
                return Err(possible_commit(
                    "ended with a status nothing declares, or by a signal",
                ));
            }
            None => {
                return Err(answered(
                    &codes::row::UNEXPECTED_EXIT_STATUS,
                    match last.finished.exit {
                        Exit::Code(code) => format!(
                            "The command `{name}` ended with the status {code}, which its `exits` do not declare."
                        ),
                        Exit::Killed => format!("The command `{name}` was ended by a signal."),
                    },
                    "Find out what the status means and declare it under `exits` in `commands/`.",
                ));
            }
        };
        if let Some(last) = session.last.as_mut() {
            last.done = Some(done);
        }
        Ok(if done {
            StepOutcome::Done(Observed::default())
        } else {
            StepOutcome::Absent(Observed::default())
        })
    }

    fn parse(&self, session: &Session, step: &Step) -> Result<StepOutcome, StepFailure> {
        let (name, command, _) =
            found(session, step).map_err(|failure| unconfirmed(session, failure))?;
        let one = match step
            .payload
            .decode()
            .map_err(|failure| unconfirmed(session, StepFailure::not_sent(failure)))?
        {
            StepPayload::Command { one, .. } => one,
            StepPayload::Probe { .. } => false,
        };
        let last = session.last.as_ref().filter(|last| last.command == name);
        let Some((last, done)) = last.and_then(|last| last.done.map(|done| (last, done))) else {
            return Err(unconfirmed(
                session,
                not_usable("its program's exit was not checked first"),
            ));
        };
        if !done {
            return Ok(StepOutcome::Absent(Observed::default()));
        }
        let writes = command.effect != Effect::Read;
        let unparsed = |why: &str| {
            if writes {
                possible_commit("printed something other than its declared output")
            } else {
                answered(
                    &codes::row::OUTPUT_UNPARSED,
                    format!(
                        "The output of the command `{name}` is not of its declared form: {why}."
                    ),
                    "Compare what the program prints now with the command's `output` in `commands/`, and correct the file if the program changed.",
                )
            }
        };
        let Ok(text) = std::str::from_utf8(&last.finished.stdout) else {
            return Err(unparsed("it is not UTF-8"));
        };
        // Redacted before it is read and again after: reading can put a
        // secret together that the program printed in pieces.
        let parsed = output::read(self.redact.as_ref(), text, &command.output)
            .map_err(|why| unparsed(&why))?;
        let values = match parsed {
            Parsed::Record(values) => values,
            Parsed::Rows(mut rows) if one => match rows.len() {
                0 if step.lookup => return Ok(StepOutcome::Absent(Observed::default())),
                1 => rows.remove(0),
                0 => return Err(unparsed("it holds no record")),
                _ => {
                    return Err(answered(
                        &codes::row::AMBIGUOUS_MATCH,
                        format!(
                            "The command `{name}` found more than one record where exactly one is the record written."
                        ),
                        "Find the records by hand, and decide which is the one that was written.",
                    ));
                }
            },
            Parsed::Rows(rows) => Values::new().with(
                "rows",
                rows.into_iter().map(Value::from).collect::<Vec<Value>>(),
            ),
        };
        let assigned_key = command
            .assigns
            .as_ref()
            .and_then(|field| values.get(field))
            .filter(|value| !value.is_null())
            .and_then(|value| RecordKey::new(vec![value.clone()]).ok());
        if command.assigns.is_some() && assigned_key.is_none() {
            return Err(unparsed("it does not hold the key the program assigned"));
        }
        Ok(StepOutcome::Done(Observed {
            values,
            assigned_key,
            ..Observed::default()
        }))
    }
}

/// The names copied from the engine's own environment for one start.
fn passed(program: Option<&Program>) -> Vec<String> {
    let pass: Vec<String> = program
        .map(|program| program.env_pass.clone())
        .unwrap_or_default();
    // A process on Windows cannot load its system libraries without it.
    #[cfg(windows)]
    let pass = {
        let mut pass = pass;
        if !pass
            .iter()
            .any(|name| name.eq_ignore_ascii_case("SystemRoot"))
        {
            pass.push("SystemRoot".to_owned());
        }
        pass
    };
    pass
}

/// The version a finished version command reported, when it ended as
/// declared and its output matches the declared pattern. `text` is its
/// standard output, redacted whole: the version is a piece of it, and a
/// piece is never taken from text the redactor has not seen.
fn reported_version(check: &VersionCheck, finished: &Finished, text: &str) -> Option<String> {
    if !matches!(finished.exit, Exit::Code(code) if check.exits.contains(&code))
        || stdout_cut(finished)
    {
        return None;
    }
    let line = text
        .strip_suffix('\n')
        .map_or(text, |line| line.strip_suffix('\r').unwrap_or(line));
    pattern::whole(&check.stdout)
        .ok()?
        .captures(line)?
        .name("version")
        .map(|version| version.as_str().to_owned())
}

/// The command a step refers to, with its program, when both belong to the
/// session's surface.
fn found<'a>(
    session: &'a Session,
    step: &'a Step,
) -> Result<(&'a str, &'a Command, &'a Program), StepFailure> {
    let entry = step
        .refers_to
        .as_ref()
        .map(|reference| reference.entry.as_str())
        .ok_or_else(|| not_usable("it refers to no command"))?;
    let command = session
        .section
        .commands
        .get(entry)
        .ok_or_else(|| not_usable("the plan holds no command of that name"))?;
    let program = session
        .section
        .programs
        .get(command.program as usize)
        .filter(|program| program.surface == session.surface)
        .ok_or_else(|| not_usable("its command belongs to another surface"))?;
    Ok((entry, command, program))
}

/// The most bytes a script the target keeps may have.
const MOST_SCRIPT_BYTES: u64 = 1024 * 1024;

/// Reads the script of the command `name` from the target's folder, checks
/// its bytes against the declared digest, and writes them to a file in the
/// run's own folder of scripts, whose path it gives. The copy is read back
/// and checked again: the interpreter is handed the bytes that were
/// checked, in a folder no other user may write, whatever happens to the
/// target's own file after this.
///
/// That no other user may write it is checked, not assumed: the copy, its
/// folder, and every folder on the way to it are held to the provider's
/// check as a file the engine wrote, after the copy is read back and
/// immediately before the interpreter is started. Nobody but the engine's
/// user and the administrator can then change what the interpreter opens.
fn script_copy(
    session: &Session,
    name: &str,
    script: &Script,
    trust: &Trust,
) -> Result<String, StepFailure> {
    let (Some(project), Some(folder)) = (session.project.as_ref(), session.scripts.as_ref()) else {
        return Err(StepFailure::not_sent(a2m_provider::codes::row::PROJECT_FOLDER_MISSING.failure(
            format!(
                "The command `{name}` runs a script the target keeps, and the call that started the run came with no project folder to find the target in."
            ),
            "Call the tool from inside the project that holds the target.",
        )));
    };
    let shown_path = shown(&script.path);
    let missing = || {
        not_sent(
            &codes::row::PROGRAM_MISSING,
            format!(
                "The script `{shown_path}` of the command `{name}` is not in the target's folder, or cannot be read."
            ),
            "Put the script back at the path its command names, inside the target's folder.",
        )
    };
    let changed = |place: &str| {
        not_sent(
            &codes::row::PROGRAM_DIGEST_CHANGED,
            format!(
                "The script `{shown_path}` of the command `{name}` {place} is not the one its digest names."
            ),
            "Review the script, write the SHA-256 of its bytes under the command's `script.digest`, and publish.",
        )
    };
    let original = project
        .join(&format!("targets/{}/{}", session.target, script.path))
        .map_err(StepFailure::not_sent)?;
    let bytes = original.read(MOST_SCRIPT_BYTES).map_err(|_| missing())?;
    if Digest::of(&bytes).to_string() != script.digest {
        return Err(changed("in the target's folder"));
    }
    let file_name = script.path.rsplit('/').next().unwrap_or(&script.path);
    let copy = folder
        .join(&format!("{name}-{file_name}"))
        .map_err(StepFailure::not_sent)?;
    copy.write(&bytes).map_err(StepFailure::not_sent)?;
    let written = copy.read(MOST_SCRIPT_BYTES).map_err(|_| missing())?;
    if Digest::of(&written).to_string() != script.digest {
        return Err(changed("as copied for the run"));
    }
    if let Err(why) = safety::check(copy.as_path(), trust, Role::Script) {
        let what = match &why {
            Refused::Writable(at) | Refused::Unreadable(at) | Refused::Link(at) => {
                shown(&at.display().to_string())
            }
            _ => shown(&copy.as_path().display().to_string()),
        };
        return Err(not_sent(
            &codes::row::PROGRAM_PATH_UNSAFE,
            format!(
                "The copy of the script `{shown_path}` of the command `{name}` is not started: another user could change it, or who may change `{what}` could not be read."
            ),
            "Keep the command pack's `command_runs` folder, and every folder above it, to the engine's user alone, with no write access for anyone else.",
        ));
    }
    copy.as_path()
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| not_usable("the run's folder of scripts has a path that is not text"))
}

/// How a program ended, as a word for the trace's signature: its status, or
/// that a signal ended it.
fn exit_word(exit: &Exit) -> String {
    match exit {
        Exit::Code(code) => code.to_string(),
        Exit::Killed => "signal".to_owned(),
    }
}

/// Removes a run's folder: its files, then the folder itself.
fn remove_folder(folder: &CheckedPath) -> bool {
    if let Ok((entries, _)) = folder.list() {
        for entry in entries {
            let _ = entry.remove_file();
        }
    }
    // Only an empty folder is removed, so nothing a program left in a
    // folder of its own making is followed.
    std::fs::remove_dir(folder.as_path()).is_ok() || !folder.exists()
}

impl Driver for CliDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move { self.prepare_session(cx).await.map(Prepared::new) })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let Some(session) = prepared.state::<Session>() else {
                return Err(not_usable("the driver was not prepared for it"));
            };
            if step.kind == codes::SPAWN {
                self.spawn(session, step, input).await
            } else if step.kind == codes::EXPECT {
                self.expect(session, step)
            } else if step.kind == codes::PARSE {
                self.parse(session, step)
            } else {
                Err(not_usable("the mode has no step of that kind"))
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            let Ok(session) = prepared.into_state::<Session>() else {
                return Cleanup::complete();
            };
            let scripts_gone = session.scripts.as_ref().is_none_or(remove_folder);
            if remove_folder(&session.workdir) && scripts_gone {
                Cleanup::complete()
            } else {
                Cleanup::incomplete(vec![Leftover {
                    kind: None,
                    what: format!(
                        "the run's folder `{}` holds what the program left and could not be removed",
                        session.workdir
                    ),
                }])
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{Digest, FailureClass};
    use a2m_provider::{Cleanup, Exit, Finished, Leftover, Sent};

    use super::{left_running, reported_version, stdout_cut};
    use crate::codes::{CLEANUP_INCOMPLETE, NO_CONFIRMATION};
    use crate::model::VersionCheck;

    fn finished(stdout: &str, wrote: u64, left: bool) -> Finished {
        Finished {
            exit: Exit::Code(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            stdout_bytes: wrote,
            stderr_bytes: 0,
            program_digest: Digest::of(b"example"),
            verified: true,
            cleanup: if left {
                Cleanup::incomplete(vec![Leftover {
                    kind: None,
                    what: "a process group that could not be ended".to_owned(),
                }])
            } else {
                Cleanup::complete()
            },
        }
    }

    #[test]
    fn a_program_that_left_something_running_has_not_finished() {
        assert!(left_running(&finished("", 0, false), true).is_none());
        // A write that ended with `ok` while what it started still runs is
        // not confirmed: the record is read back before anything else.
        let write = left_running(&finished("", 0, true), true).unwrap();
        assert_eq!(
            (write.failure.class(), write.failure.code(), write.sent),
            (
                FailureClass::PossibleCommit,
                &NO_CONFIRMATION,
                Sent::Unknown
            )
        );
        let read = left_running(&finished("", 0, true), false).unwrap();
        assert_eq!(
            (read.failure.class(), read.failure.code(), read.sent),
            (FailureClass::Busy, &CLEANUP_INCOMPLETE, Sent::Answered)
        );
    }

    #[test]
    fn a_version_is_read_only_from_whole_output_the_redactor_has_seen() {
        let check = VersionCheck {
            args: vec!["--version".to_owned()],
            exits: vec![0],
            stdout: r"^example-tool (?<version>\S+)$".to_owned(),
            supported: Vec::new(),
        };
        let whole = finished("example-tool 2.4\n", 17, false);
        assert_eq!(
            reported_version(&check, &whole, "example-tool 2.4\n").as_deref(),
            Some("2.4")
        );
        // The text is the redacted one: a version is a piece of it.
        assert_eq!(
            reported_version(&check, &whole, "example-tool [redacted]\n").as_deref(),
            Some("[redacted]")
        );
        // Output the supervisor cut is never read.
        let cut = finished("example-tool 2.4\n", 4096, false);
        assert!(stdout_cut(&cut));
        assert_eq!(reported_version(&check, &cut, "example-tool 2.4\n"), None);
    }
}
