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
    Digest, EnvironmentKind, Failure, FailureClass, RecordKey, Redact, Step, Value, Values,
};
use a2m_provider::{
    Access, BoxFuture, Capture, CheckedPath, Cleanup, ConnFailure, Deadline, Driver, DriverHealth,
    EnvAllowlist, Exit, Finished, LaunchSpec, Leftover, Observed, OneShot, Posture, PrepareContext,
    Prepared, ProgramRef, Ran, RootName, Roots, ScopedSupervisor, StepFailure, StepInput,
    StepOutcome, SubsystemKind, SurfaceRef, current_platform,
};

use crate::args::{self, Given};
use crate::codes::{
    self, AMBIGUOUS_MATCH, APP_VALIDATION_ERROR, FILE_KIND, LIMIT_EXCEEDED, NEEDS_LOGIN,
    OUTPUT_UNPARSED, PACK, PROGRAM_BUSY, PROGRAM_DIGEST_CHANGED, PROGRAM_MISSING,
    PROGRAM_PATH_UNSAFE, PROGRAM_VERSION_UNSUPPORTED, READ_ONLY_POSTURE, RECORD_ABSENT,
    SCHEMA_INVALID, SURFACE_KIND, UNEXPECTED_EXIT_STATUS, UNEXPECTED_PROMPT, UPSTREAM_UNAVAILABLE,
    VERSION_CONFLICT, answered, not_sent, possible_commit, shown,
};
use crate::model::{
    Command, Effect, ExitMeaning, Limits, Probe, Program, Section, StepPayload, VersionCheck,
};
use crate::output::{self, Parsed};
use crate::pattern;
use crate::safety::{self, Refused};

/// The root the pack makes one fresh folder in for each run.
pub(crate) const RUNS_ROOT: &str = "command_runs";
/// The kind of subsystem a started program is counted as.
pub(crate) const SUBSYSTEM: &str = "command";
/// How long a help or version form may run.
const PROBE_LIMIT_MS: u32 = 10_000;

/// The driver of the `cli` mode.
pub(crate) struct CliDriver {
    pub roots: Roots,
    pub redact: Arc<dyn Redact>,
    pub runs_root: RootName,
    pub kind: SubsystemKind,
    pub max_timeout_ms: u64,
    pub max_output_bytes: u64,
    pub next_folder: AtomicU64,
}

/// How the last start of a program ended, kept between its three steps.
struct Last {
    command: String,
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
            FailureClass::Drift,
            PROGRAM_MISSING,
            format!("The program `{at}` is not there, is not a file, or cannot be run."),
            "Install the program at the path its surface declares, or correct the path in `target.yaml`.",
        ),
        Refused::NotAbsolute => not_sent(
            FailureClass::Policy,
            PROGRAM_PATH_UNSAFE,
            format!("The program `{at}` is not named by an absolute path."),
            "Name the program by its absolute path in `target.yaml`; it is never searched for.",
        ),
        Refused::Script => not_sent(
            FailureClass::Policy,
            PROGRAM_PATH_UNSAFE,
            format!("The program `{at}` is a shell, an interpreter, or a script that one of them would run."),
            "Name a program that takes its arguments as data; a shell or a script is never started.",
        ),
        Refused::Writable(what) => not_sent(
            FailureClass::Policy,
            PROGRAM_PATH_UNSAFE,
            format!(
                "The program `{at}` could be replaced by another user: `{}` is theirs to write.",
                shown(&what.display().to_string())
            ),
            "Give the file and every folder above it to the engine's user or the administrator, with no write access for anyone else.",
        ),
    }
}

fn digest_changed(path: &Path, why: &str) -> StepFailure {
    not_sent(
        FailureClass::Drift,
        PROGRAM_DIGEST_CHANGED,
        format!(
            "The program `{}` {why}.",
            shown(&path.display().to_string())
        ),
        "Review the program that is installed now, add its SHA-256 under `program.digests` for this platform, and publish.",
    )
}

/// The failure a step raises when the plan does not hold together for it.
fn as_written(step: &Step, what: &str) -> StepFailure {
    StepFailure::not_sent(Failure::new(
        step.fails_as.class,
        step.fails_as.code.clone(),
        format!("The `{}` step cannot be done as written: {what}.", step.kind),
        "Compile the target again with the pack that is enabled now.",
    ))
}

impl CliDriver {
    fn text(&self, bytes: &[u8]) -> String {
        self.redact.redact(&String::from_utf8_lossy(bytes))
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
    ) -> Result<Finished, StepFailure> {
        let allowed_ms = u64::from(limits.timeout_ms)
            .min(self.max_timeout_ms)
            .min(u64::try_from(deadline.remaining().as_millis()).unwrap_or(u64::MAX));
        if allowed_ms == 0 {
            return Err(not_sent(
                FailureClass::Transient,
                codes::TIMEOUT,
                "No time was left to start the program.",
                "Try again.",
            ));
        }
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut pass: Vec<String> = program
            .map(|program| program.env_pass.clone())
            .unwrap_or_default();
        // A process on Windows cannot load its system libraries without it.
        #[cfg(windows)]
        if !pass.iter().any(|name| name.eq_ignore_ascii_case("SystemRoot")) {
            pass.push("SystemRoot".to_owned());
        }
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
            Ok(finished) => Ok(finished),
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
                FailureClass::Busy,
                PROGRAM_BUSY,
                "Another start of the program holds the surface.",
                "Try again when it has ended.",
            );
        }
        if failure.code() == &contract::EFFECT_NOT_DECLARED {
            let read = path.clone();
            let now = joined(move || safety::digest_of(&read).ok()).await.flatten();
            if now != Some(session.digest) {
                return digest_changed(&path, "changed after it was checked: its digest is another now");
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
            .start(session, check.args.clone(), None, limits, deadline)
            .await?;
        let unsupported = |why: &str| {
            answered(
                FailureClass::Drift,
                PROGRAM_VERSION_UNSUPPORTED,
                format!(
                    "The program `{}` {why}.",
                    shown(&session.path.display().to_string())
                ),
                "Install a supported version, or check the target's files against this one and add it to `program.version.supported`.",
            )
        };
        match reported_version(check, &finished) {
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
        let checked = path.clone();
        let verdict = joined(move || {
            safety::check(&checked).map(|()| safety::digest_of(&checked).ok())
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

        // A fresh, empty folder for this run.
        let number = self.next_folder.fetch_add(1, Ordering::Relaxed);
        let run: String = cx
            .run
            .to_string()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let workdir = self
            .roots
            .folder(&self.runs_root, Access::Write)
            .and_then(|root| root.join(&format!("{run}-{}-{number}", std::process::id())))
            .and_then(|folder| folder.create_dir_all().map(|()| folder))
            .map_err(StepFailure::not_sent)?;

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
                return Err(failure);
            }
        }
        Ok(session)
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
                    word.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                        && word
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
                };
                if !words.iter().all(plain) {
                    return Err(not_sent(
                        FailureClass::Input,
                        codes::ARGUMENT_UNSAFE,
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
            .start(session, elements, None, limits, input.deadline)
            .await?;
        let mut values = Values::new()
            .with("stdout", self.text(&finished.stdout))
            .with(
                "elements",
                shown_elements.into_iter().map(Value::from).collect::<Vec<Value>>(),
            )
            .with("digest", session.digest.to_string())
            .with("truncated", finished.truncated());
        if let Exit::Code(code) = finished.exit {
            values.insert("exit", i64::from(code));
        }
        if let (Probe::Version, Some(check)) = (probe, &program.version)
            && let Some(version) = reported_version(check, &finished)
        {
            values.insert("pattern", check.stdout.clone());
            values.insert("version", self.redact.redact(&version));
        }
        Ok(StepOutcome::Done(Observed {
            values,
            ..Observed::default()
        }))
    }

    async fn spawn(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let payload: StepPayload = step.payload.decode().map_err(StepFailure::not_sent)?;
        let key_params = match payload {
            StepPayload::Probe { program, probe } => {
                return self.probe(session, program, &probe, input).await;
            }
            StepPayload::Command { key_params, .. } => key_params,
        };
        let allowed = match session.posture {
            Posture::Live => true,
            Posture::Sandbox { .. } => session.test_environment,
            Posture::Discovery => false,
        };
        if !allowed {
            return Err(not_sent(
                FailureClass::Policy,
                READ_ONLY_POSTURE,
                "A command of a program was asked for where only its declared help and version forms may run: nothing can make a program unable to write.",
                "Try the command against an environment of kind `test`, or run the operation live.",
            ));
        }
        let (name, command, program) = found(session, step)?;
        let name = name.to_owned();
        let elements = args::elements(
            command,
            &Given {
                values: input.args,
                key_params: &key_params,
                key: input.key,
                stamp: input.stamp,
            },
        )?;
        let writes = command.effect != Effect::Read;
        let limits = command.limits;
        let finished = self
            .start(session, elements, Some(program), limits, input.deadline)
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
                    FailureClass::Policy,
                    LIMIT_EXCEEDED,
                    format!(
                        "The command `{name}` printed more than its ceiling `limits.{stream}` of {ceiling} bytes."
                    ),
                    "Narrow what the command lists, or raise `limits` in its `commands/` file and the setting `packs.command.max_output_bytes`.",
                )
            });
        }
        let captures = vec![
            self.capture("stdout", &finished.stdout),
            self.capture("stderr", &finished.stderr),
        ];
        session.last = Some(Last {
            command: name,
            finished,
            done: None,
        });
        Ok(StepOutcome::Done(Observed {
            captures,
            ..Observed::default()
        }))
    }

    fn expect(&self, session: &mut Session, step: &Step) -> Result<StepOutcome, StepFailure> {
        let (name, command, program) = found(session, step)?;
        let Some(last) = session.last.as_ref().filter(|last| last.command == name) else {
            return Err(as_written(step, "its program was not started first"));
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
                    FailureClass::UnknownVariant,
                    UNEXPECTED_PROMPT,
                    format!("The command `{name}` asked a question, and a program is never given a person to answer one."),
                    "Add the argument that makes the command not ask, such as `--yes`, to its `args` in `commands/`.",
                )
            });
        }
        let meaning = match last.finished.exit {
            Exit::Code(code) => command.exits.get(&code.to_string()).copied(),
            Exit::Killed => None,
        };
        // What the program said on standard error, cut, cleaned, and
        // redacted. It is untrusted text and is marked as such.
        let said = self.redact.redact(&shown(stderr.trim()));
        let own = |class, code, what: &str, next: &str| {
            answered(
                class,
                code,
                format!("The command `{name}` {what}. Its own message, which is untrusted text: \"{said}\"."),
                next,
            )
        };
        let done = match meaning {
            Some(ExitMeaning::Ok) => true,
            Some(ExitMeaning::Absent) if step.lookup => false,
            Some(ExitMeaning::Absent) => {
                return Err(own(
                    FailureClass::Stale,
                    RECORD_ABSENT,
                    "found no record of that name, and changed nothing",
                    "Read the record again; it may have been removed.",
                ));
            }
            Some(ExitMeaning::Input) => {
                return Err(own(
                    FailureClass::Input,
                    SCHEMA_INVALID,
                    "refused its arguments as badly formed, and changed nothing",
                    "Correct the values and call again.",
                ));
            }
            Some(ExitMeaning::Rejected) => {
                return Err(own(
                    FailureClass::Rejected,
                    APP_VALIDATION_ERROR,
                    "refused the values by a rule of its own, and changed nothing",
                    "Correct the values and call again.",
                ));
            }
            Some(ExitMeaning::Stale) => {
                return Err(own(
                    FailureClass::Stale,
                    VERSION_CONFLICT,
                    "found the record changed by someone else, and changed nothing",
                    "Read the record again and decide whether the change still applies.",
                ));
            }
            Some(ExitMeaning::Busy) => {
                return Err(own(
                    FailureClass::Busy,
                    PROGRAM_BUSY,
                    "found what it needs held by another, and changed nothing",
                    "Try again shortly.",
                ));
            }
            Some(ExitMeaning::Transient) => {
                return Err(own(
                    FailureClass::Transient,
                    UPSTREAM_UNAVAILABLE,
                    "asked to be tried again later, and changed nothing",
                    "Try again shortly.",
                ));
            }
            Some(ExitMeaning::SessionExpired) => {
                return Err(own(
                    FailureClass::SessionExpired,
                    NEEDS_LOGIN,
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
                    FailureClass::UnknownVariant,
                    UNEXPECTED_EXIT_STATUS,
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
        let (name, command, _) = found(session, step)?;
        let one = match step.payload.decode().map_err(StepFailure::not_sent)? {
            StepPayload::Command { one, .. } => one,
            StepPayload::Probe { .. } => false,
        };
        let last = session.last.as_ref().filter(|last| last.command == name);
        let Some((last, done)) = last.and_then(|last| last.done.map(|done| (last, done))) else {
            return Err(as_written(step, "its program's exit was not checked first"));
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
                    FailureClass::Drift,
                    OUTPUT_UNPARSED,
                    format!("The output of the command `{name}` is not of its declared form: {why}."),
                    "Compare what the program prints now with the command's `output` in `commands/`, and correct the file if the program changed.",
                )
            }
        };
        let Ok(text) = std::str::from_utf8(&last.finished.stdout) else {
            return Err(unparsed("it is not UTF-8"));
        };
        // Redacted before it is read, so no value can hold a secret.
        let text = self.redact.redact(text);
        let parsed = output::parse(&text, &command.output).map_err(|why| unparsed(&why))?;
        let values = match parsed {
            Parsed::Record(values) => values,
            Parsed::Rows(mut rows) if one => match rows.len() {
                0 if step.lookup => return Ok(StepOutcome::Absent(Observed::default())),
                1 => rows.remove(0),
                0 => return Err(unparsed("it holds no record")),
                _ => {
                    return Err(answered(
                        FailureClass::PossibleCommit,
                        AMBIGUOUS_MATCH,
                        format!("The command `{name}` found more than one record where exactly one is the record written."),
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

/// The version a finished version command reported, when it ended as
/// declared and its output matches the declared pattern.
fn reported_version(check: &VersionCheck, finished: &Finished) -> Option<String> {
    if !matches!(finished.exit, Exit::Code(code) if check.exits.contains(&code)) {
        return None;
    }
    let text = String::from_utf8_lossy(&finished.stdout);
    let line = text
        .strip_suffix('\n')
        .map_or(&*text, |line| line.strip_suffix('\r').unwrap_or(line));
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
        .ok_or_else(|| as_written(step, "it refers to no command"))?;
    let command = session
        .section
        .commands
        .get(entry)
        .ok_or_else(|| as_written(step, "the plan holds no command of that name"))?;
    let program = session
        .section
        .programs
        .get(command.program as usize)
        .filter(|program| program.surface == session.surface)
        .ok_or_else(|| as_written(step, "its command belongs to another surface"))?;
    Ok((entry, command, program))
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
                return Err(as_written(step, "the driver was not prepared for it"));
            };
            if step.kind == codes::SPAWN {
                self.spawn(session, step, input).await
            } else if step.kind == codes::EXPECT {
                self.expect(session, step)
            } else if step.kind == codes::PARSE {
                self.parse(session, step)
            } else {
                Err(as_written(step, "the mode has no step of that kind"))
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
            if remove_folder(&session.workdir) {
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
