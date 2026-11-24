//! The optional decompiling engine, as a provider of discovery (package X6,
//! decision 0343).
//!
//! A person installs an engine; the product never fetches one. The
//! [`Decompiler`] holds where the person put it and the digests the product
//! pins for it. Before every start it takes the digest of the file at that
//! place and refuses one that is not pinned. It starts the program through
//! the supervisor, with the digest it found as the digest that must run, and
//! hands it a file as one argument: a [`CheckedPath`] of a regular file,
//! never a process, so there is nothing here to attach to. The file it was
//! handed is never started, loaded, or opened for writing.
//!
//! What the engine prints is untrusted text. It is read as lines of JSON in
//! the one form below, and a line in any other form is a named stop. It
//! becomes findings of the source `artifact`, which supports no level above
//! `inferred`; the text is listed apart by the describer, and nothing here
//! can reach a run, because a finding is evidence for a person or an agent
//! that drafts, and no compiled target or step reads one.
//!
//! The form, one JSON object to a line, in this order: any number of
//! `{"function": text, "address": text, "text": text}`, then once
//! `{"end": true, "functions": number}`. A last line that is missing is a
//! stop unless the output was cut at its bound, which is said.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Coverage, Digest, EvidenceLevel, Failure, Finding, FindingKind, LimitationCode, ProducedBy,
    Redact, Source, Subject, Values,
};
use a2m_telemetry::Clock;

use crate::codes;
use crate::discover::{
    DescribeForm, Describer, Description, Discoverer, DiscoveryReport, DiscoveryScope, FindingSink,
    SandboxPort, SurfaceDescription,
};
use crate::name::{DiscovererName, SubsystemKind};
use crate::path::CheckedPath;
use crate::supervisor::{
    EnvAllowlist, Exit, LaunchSpec, OneShot, ProgramRef, ProgramRefused, ProgramRole, ProgramTrust,
    Supervisor, check_program,
};
use crate::{BoxFuture, EntryKind};

/// The argument of a [`DecompilerSpec`] that is replaced by the path of the
/// file the engine is handed. It stands alone as an argument: a part of an
/// argument is not replaced.
pub const FILE_ARGUMENT: &str = "{file}";

/// The name a file is handed to the discoverer under, in the project's
/// `sources` folder.
pub const FILE_NAME: &str = "decompile";

/// The name of the discoverer.
pub const DISCOVERER: &str = "decompile";

/// The kind of finding it makes: one for each function the engine made text
/// of.
pub const FINDING_KIND: &str = "decompiled_function";

/// The most a program that is the engine may be, in bytes, to be digested.
const MOST_PROGRAM_BYTES: u64 = 256 * 1024 * 1024;

/// What one engine is, as the product states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompilerSpec {
    /// The engine's name, as the findings say who made them.
    pub name: String,
    /// The digests of the program the product accepts as this engine. A
    /// program whose digest is none of these is not started.
    pub pinned: Vec<Digest>,
    /// The program's arguments, in which exactly one is [`FILE_ARGUMENT`].
    pub args: Vec<String>,
    /// The variables the program is started with, and no others. Plain
    /// settings of the engine, never a secret.
    pub env: Vec<(String, OsString)>,
    /// How long one run may take before its whole group is ended.
    pub time_limit: Duration,
    /// The most bytes of its output that are kept; what comes after is
    /// counted and dropped.
    pub max_output_bytes: u64,
}

/// Where the program stands, as far as can be told without starting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// The person has named no engine.
    NotInstalled,
    /// The person named a program and nothing usable is there.
    Missing(PathBuf),
    /// A program is there and its digest is not pinned.
    NotPinned {
        /// Where it is.
        path: PathBuf,
        /// Its digest.
        digest: Digest,
    },
    /// A program is there and its digest is pinned.
    Pinned {
        /// Where it is.
        path: PathBuf,
        /// Its digest.
        digest: Digest,
    },
}

/// The engine the person installed, if any.
#[derive(Debug, Clone)]
pub struct Decompiler {
    spec: DecompilerSpec,
    installed: Option<PathBuf>,
    supervisor: Supervisor,
    trust: ProgramTrust,
}

/// What an engine made of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decompiled {
    /// The functions it made text of, in the order it printed them, up to
    /// the bound asked for.
    pub functions: Vec<DecompiledFunction>,
    /// How many it printed beyond the bound asked for, which are dropped.
    pub over: u64,
    /// Whether its output was cut at the bound, so that the last of its
    /// functions may be missing.
    pub cut: bool,
    /// The digest of the program that ran, which is a pinned one.
    pub engine: Digest,
}

/// One function, as the engine printed it. All of it is the engine's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompiledFunction {
    /// The function's name.
    pub function: String,
    /// Its address, as text.
    pub address: String,
    /// The C text the engine made.
    pub text: String,
}

impl Decompiler {
    /// An engine of `spec`, installed at `installed` when the person named
    /// a place, started through `supervisor`. `trust` is who the engine's
    /// user trusts to change a program, as for any program the product
    /// starts.
    ///
    /// # Errors
    ///
    /// `input`/`decompiler_spec_invalid` for a spec with no pinned digest, or
    /// whose arguments do not hold [`FILE_ARGUMENT`] exactly once.
    pub fn new(
        spec: DecompilerSpec,
        installed: Option<PathBuf>,
        supervisor: Supervisor,
        trust: ProgramTrust,
    ) -> Result<Self, Failure> {
        let places = spec.args.iter().filter(|a| *a == FILE_ARGUMENT).count();
        if spec.pinned.is_empty() || places != 1 {
            return Err(codes::failure(
                &codes::row::DECOMPILER_SPEC_INVALID,
                format!(
                    "The decompiler `{}` must pin at least one digest and name `{FILE_ARGUMENT}` once among its arguments.",
                    codes::shown(&spec.name)
                ),
                "State the digests of the engine's releases and one `{file}` argument.",
            ));
        }
        Ok(Self {
            spec,
            installed,
            supervisor,
            trust,
        })
    }

    /// The spec.
    #[must_use]
    pub fn spec(&self) -> &DecompilerSpec {
        &self.spec
    }

    /// Looks at the program, and starts nothing: whether the person named
    /// one, whether it is there, and whether its digest is pinned.
    pub async fn identify(&self) -> Installed {
        let Some(path) = self.installed.clone() else {
            return Installed::NotInstalled;
        };
        let read = {
            let path = path.clone();
            tokio::task::spawn_blocking(move || {
                let meta = std::fs::metadata(&path).ok()?;
                if !meta.is_file() || meta.len() > MOST_PROGRAM_BYTES {
                    return None;
                }
                std::fs::read(&path).ok()
            })
            .await
            .ok()
            .flatten()
        };
        match read {
            None => Installed::Missing(path),
            Some(bytes) => {
                let digest = Digest::of(&bytes);
                if self.spec.pinned.contains(&digest) {
                    Installed::Pinned { path, digest }
                } else {
                    Installed::NotPinned { path, digest }
                }
            }
        }
    }

    /// Hands the engine `file` and takes back what it printed.
    ///
    /// # Errors
    ///
    /// `busy`/`decompiler_not_installed` with no engine or none there;
    /// `policy`/`decompiler_not_pinned` for a program whose digest is not
    /// pinned or that changed after it was looked at;
    /// `policy`/`effect_not_declared` for a program this system's rules for
    /// who may change a program refuse; the supervisor's own failures;
    /// `rejected`/`decompiler_failed` for an exit that is not success;
    /// `unknown_variant`/`decompiler_output_unknown` for output in a form
    /// that is not the one above.
    pub async fn decompile(
        &self,
        file: &CheckedPath,
        most_functions: u64,
    ) -> Result<Decompiled, Failure> {
        if file.entry_kind()? != EntryKind::File {
            return Err(codes::failure(
                &codes::row::FILE_UNAVAILABLE,
                "A decompiler is handed a regular file and nothing else.",
                "Hand the file itself, from the project's `sources` folder.",
            ));
        }
        let (path, digest) = match self.identify().await {
            Installed::Pinned { path, digest } => (path, digest),
            Installed::NotInstalled => {
                return Err(codes::failure(
                    &codes::row::DECOMPILER_NOT_INSTALLED,
                    format!(
                        "No decompiling engine is installed: no program is named for `{}`. The scout goes on without decompiled text.",
                        codes::shown(&self.spec.name)
                    ),
                    "Install the engine yourself and name its program in the settings; the product never fetches one.",
                ));
            }
            Installed::Missing(path) => {
                return Err(codes::failure(
                    &codes::row::DECOMPILER_NOT_INSTALLED,
                    format!(
                        "No decompiling engine is at `{}`. The scout goes on without decompiled text.",
                        codes::shown(&path.to_string_lossy())
                    ),
                    "Install the engine at the place the settings name, or correct the setting.",
                ));
            }
            Installed::NotPinned { path, digest } => {
                return Err(not_pinned(&path, &digest));
            }
        };
        let checked_path = path.clone();
        let trust = self.trust.clone();
        let verdict = tokio::task::spawn_blocking(move || {
            check_program(&checked_path, &trust, ProgramRole::Program)
        })
        .await
        .map_err(|_| {
            codes::failure(
                &codes::row::START_FAILED,
                "The check of the engine's program did not finish.",
                "Report this as a defect of the engine.",
            )
        })?;
        if let Err(refused) = verdict {
            return Err(program_refused(&path, &refused));
        }
        let args = self
            .spec
            .args
            .iter()
            .map(|arg| {
                if arg == FILE_ARGUMENT {
                    file.as_path().as_os_str().to_owned()
                } else {
                    arg.into()
                }
            })
            .collect();
        let launch = LaunchSpec {
            program: ProgramRef {
                path: path.clone(),
                digest: Some(digest),
            },
            args,
            env: EnvAllowlist {
                pass: Vec::new(),
                set: self.spec.env.clone(),
            },
            cwd: None,
            kind: SubsystemKind::new("decompiler").map_err(|_| {
                codes::failure(
                    &codes::row::START_FAILED,
                    "The decompiler's subsystem kind is not a name.",
                    "Report this as a defect of the engine.",
                )
            })?,
            place: None,
        };
        let link = OneShot {
            max_output_bytes: self.spec.max_output_bytes,
            time_limit: self.spec.time_limit,
            stdin: None,
        };
        let finished = self
            .supervisor
            .run(launch, link)
            .await
            .map_err(|failed| failed.failure)?;
        if !finished.verified || finished.program_digest != digest {
            return Err(not_pinned(&path, &finished.program_digest));
        }
        if finished.exit != Exit::Code(0) {
            return Err(codes::failure(
                &codes::row::DECOMPILER_FAILED,
                format!(
                    "The decompiling engine ended without success ({}). What it printed is not used.",
                    match finished.exit {
                        Exit::Code(code) => format!("status {code}"),
                        Exit::Killed => "it was ended before it was done".to_owned(),
                    }
                ),
                "Try the file again, or hand the engine a file it can read; nothing it printed was kept.",
            ));
        }
        let cut = finished.stdout_bytes > finished.stdout.len() as u64;
        read_output(&finished.stdout, cut, most_functions, digest)
    }
}

fn not_pinned(path: &std::path::Path, digest: &Digest) -> Failure {
    codes::failure(
        &codes::row::DECOMPILER_NOT_PINNED,
        format!(
            "The program at `{}` has the digest {digest}, which is not one the product pins for this engine. It is not started.",
            codes::shown(&path.to_string_lossy())
        ),
        "Install a release of the engine the product pins, or use a product version that pins the one you have.",
    )
}

fn program_refused(path: &std::path::Path, refused: &ProgramRefused) -> Failure {
    codes::failure(
        &codes::row::EFFECT_NOT_DECLARED,
        format!(
            "The program at `{}` may not be started: {refused:?}.",
            codes::shown(&path.to_string_lossy())
        ),
        "Install the engine where only you and the system's administrators may change it, as a program of the system's own form.",
    )
}

fn unknown_output(why: &str) -> Failure {
    codes::failure(
        &codes::row::DECOMPILER_OUTPUT_UNKNOWN,
        format!(
            "What the decompiling engine printed is not in the form the product reads: {why}. Nothing of it was used."
        ),
        "Use an engine and an adapter that print one JSON object to a line, as the decompiler's documentation says.",
    )
}

/// Reads what the engine printed. Every line must be one of the two forms.
fn read_output(
    out: &[u8],
    cut: bool,
    most_functions: u64,
    engine: Digest,
) -> Result<Decompiled, Failure> {
    let mut lines: Vec<&[u8]> = out.split(|b| *b == b'\n').collect();
    // The text after the last newline is a whole line only when the output
    // was not cut; a cut one may end anywhere.
    let last = lines.pop().unwrap_or_default();
    if !last.is_empty() && !cut {
        lines.push(last);
    }
    let mut functions = Vec::new();
    let mut over = 0_u64;
    let mut ended: Option<u64> = None;
    let mut seen = 0_u64;
    for line in lines.into_iter().filter(|line| !line.is_empty()) {
        if ended.is_some() {
            return Err(unknown_output("a line follows the last line"));
        }
        let value: serde_json::Value = serde_json::from_slice(line)
            .map_err(|_| unknown_output("a line is not one JSON value"))?;
        let object = value
            .as_object()
            .ok_or_else(|| unknown_output("a line is not an object"))?;
        if object.get("end") == Some(&serde_json::Value::Bool(true)) {
            let count = object
                .get("functions")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| unknown_output("the last line gives no count"))?;
            ended = Some(count);
            continue;
        }
        let text_of = |name: &str| {
            object
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let (Some(function), Some(address), Some(text)) =
            (text_of("function"), text_of("address"), text_of("text"))
        else {
            return Err(unknown_output(
                "a function line lacks its function, address, or text",
            ));
        };
        seen += 1;
        if (functions.len() as u64) < most_functions {
            functions.push(DecompiledFunction {
                function,
                address,
                text,
            });
        } else {
            over += 1;
        }
    }
    match ended {
        Some(count) if count == seen => {}
        Some(_) => {
            return Err(unknown_output(
                "the count of the last line is not the number of functions printed",
            ));
        }
        None if cut => {}
        None => return Err(unknown_output("the last line is missing")),
    }
    Ok(Decompiled {
        functions,
        over,
        cut,
        engine,
    })
}

/// The discoverer of an engine's text. It starts the engine and nothing
/// else; the file it is handed is read as bytes, for its digest, and never
/// run.
pub struct DecompilerDiscoverer {
    decompiler: Arc<Decompiler>,
    clock: Arc<dyn Clock>,
    provider: String,
    version: String,
}

impl DecompilerDiscoverer {
    /// A discoverer over `decompiler`. `provider` and `version` say who the
    /// findings are from: the provider that registers it.
    #[must_use]
    pub fn new(
        decompiler: Arc<Decompiler>,
        clock: Arc<dyn Clock>,
        provider: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            decompiler,
            clock,
            provider: provider.into(),
            version: version.into(),
        }
    }

    async fn look(
        &self,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Some(file) = scope.paths.get(FILE_NAME) else {
            return Ok(DiscoveryReport::default());
        };
        let most = u64::from(scope.bounds.depth).saturating_mul(1000).max(1);
        let made = self.decompiler.decompile(file, most).await?;
        let data = file.read(scope.bounds.bytes)?;
        let subject_digest = Digest::of(&data);
        let kind = FindingKind::new(FINDING_KIND).map_err(|_| {
            codes::failure(
                &codes::row::DECOMPILER_OUTPUT_UNKNOWN,
                "The finding kind is not a name.",
                "Report this as a defect of the engine.",
            )
        })?;
        let partial = made.cut || made.over > 0;
        for function in &made.functions {
            let found = serde_json::json!({
                "function": scope.redact.redact(&function.function),
                "address": scope.redact.redact(&function.address),
                "text": scope.redact.redact(&function.text),
            })
            .to_string();
            let mut finding = Finding::about(
                Subject::File {
                    digest: subject_digest,
                    format: "decompiled".to_owned(),
                    part: Some(scope.redact.redact(&function.address)),
                },
                kind.clone(),
                found,
                Source::Artifact,
                ProducedBy {
                    provider: self.provider.clone(),
                    version: self.version.clone(),
                    component: Some(made.engine),
                },
                self.clock.now(),
            )
            .level(EvidenceLevel::Inferred)
            .coverage(if partial {
                Coverage::Partial
            } else {
                Coverage::Complete
            })
            .limitation(
                LimitationCode::NotExercised,
                "A decompiling engine made this text from the file's bytes. The file was never run, and the text may be wrong or incomplete; it is for a person or an agent to read while drafting, never an input to a run.",
            );
            if partial {
                finding = finding.limitation(
                    LimitationCode::Truncated,
                    "The engine printed more than was kept, or its output was cut at its bound; some functions are not here.",
                );
            }
            out.found(finding.build().map_err(|_| {
                codes::failure(
                    &codes::row::DECOMPILER_OUTPUT_UNKNOWN,
                    "A finding about a function could not be made.",
                    "Report this as a defect of the engine.",
                )
            })?)?;
        }
        Ok(DiscoveryReport {
            coverage: if partial {
                Coverage::Partial
            } else {
                Coverage::Complete
            },
            bounds_hit: if partial {
                vec!["bytes".to_owned()]
            } else {
                Vec::new()
            },
            dropped: made.over,
        })
    }
}

impl Discoverer for DecompilerDiscoverer {
    fn name(&self) -> DiscovererName {
        DiscovererName::new(DISCOVERER)
            .unwrap_or_else(|_| unreachable!("`decompile` is a well-formed discoverer name"))
    }

    fn suits(&self, _surface: &SurfaceDescription) -> bool {
        true
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.look(scope, out))
    }
}

/// Puts the findings so that an agent can read them. The engine's text is
/// untrusted and is listed apart; `text` says only how much there is.
pub struct DecompilerDescriber {
    /// The redactor, applied to the engine's text again.
    pub redact: Arc<dyn Redact>,
}

impl Describer for DecompilerDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let mut untrusted = Vec::new();
        let mut count = 0_i64;
        for finding in findings
            .iter()
            .filter(|finding| finding.kind().as_str() == FINDING_KIND)
        {
            count += 1;
            if form == DescribeForm::Full {
                untrusted.push(self.redact.redact(finding.found()));
            }
        }
        Description {
            text: format!(
                "A decompiling engine made C text of {count} functions of a file it was handed. The file was never run, and the text is the engine's and may be wrong."
            ),
            data: Values::new().with("functions", count),
            untrusted,
        }
    }
}
