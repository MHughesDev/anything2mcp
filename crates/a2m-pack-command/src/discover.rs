//! Discovery: what a program says about itself, through the help and
//! version forms its target declares and nothing else, and through the
//! completion file and the manual page a person hands it.
//!
//! The discoverer starts no command. It reads the list of commands from
//! the program's help, and each listed command's usage line, and makes one
//! finding for each: seen to be listed, never run. A fish completion file
//! handed under `completions` and a manual page handed under `manual` are
//! read as text before anything starts, and never run (decision 0284):
//! each makes a `command_file` finding that says how much of it was read,
//! and what they say of a listed command's options is put beside its usage
//! line in that command's finding, which is then `inferred`.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{
    Coverage, Digest, EvidenceLevel, FailsAs, Failure, FailureClass, Finding, LimitationCode,
    PackPayload, ProducedBy, Redact, Source, Step, Subject, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
    current_platform,
};
use a2m_telemetry::Clock;
use serde::{Deserialize, Serialize};

use crate::codes::{self, DISCOVERER, PACK, SPAWN, SURFACE_KIND, TIMEOUT};
use crate::metadata::{COMPLETIONS, FileRead, Handed, MANUAL, OptionFound, Takes};
use crate::model::{Probe, StepPayload};
use crate::{fish, manual};

/// How many `commands/` files of one target are asked for their forms.
const MOST_PROGRAMS: u32 = 16;
/// How many help forms of one program are tried.
const MOST_FORMS: u32 = 8;
/// How many listed commands are looked at.
const MOST_COMMANDS: usize = 200;

/// The version a program reported, with the declared form that asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionSeen {
    /// The arguments of the declared version form.
    pub args: Vec<String>,
    /// The declared pattern the report matched.
    pub pattern: String,
    /// The version the program reported.
    pub version: String,
}

/// What a `command` finding holds: one command a program's help lists.
/// Every text in it but `platform` and `digest` came from the program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandFound {
    /// The command's words, as the help lists them, such as `item show`.
    pub name: String,
    /// What the help says the command does.
    pub about: String,
    /// The rest of the command's usage line, after its words, when its own
    /// help was read. What each part means is inferred, not observed.
    pub usage: Option<String>,
    /// The platform the program was seen on.
    pub platform: String,
    /// The digest of the program's file.
    pub digest: String,
    /// The version the program reported, when its target declares how to
    /// ask.
    pub version: Option<VersionSeen>,
    /// The options a handed completion file or manual page states of the
    /// command, each with what every source says of it. Empty, and left
    /// out of the text, when no file says anything of it: the finding is
    /// then exactly what the help alone gives.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<OptionFound>,
}

/// One option of the program as a whole, as a handed file states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramOption {
    /// Every name the file gives it.
    pub flags: Vec<String>,
    /// Whether it takes a value.
    pub takes: Takes,
    /// The value's name, where the file gives one.
    pub value: Option<String>,
    /// What the file says it does, redacted.
    pub about: Option<String>,
}

/// What a `command_file` finding holds: one file handed to the scout, and
/// how much of it was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandFileFound {
    /// The name it was handed under: `completions` or `manual`.
    pub name: String,
    /// The form it was read in: `fish`, `man`, or `mdoc`.
    pub form: String,
    /// How many lines (fish) or option entries (manual page) were read.
    pub read: u64,
    /// How many were not, by reason. Nothing that was not read was run.
    pub unread: BTreeMap<String, u64>,
    /// The options it states of the program as a whole.
    pub program_options: Vec<ProgramOption>,
    /// The words of commands it speaks of that the help does not list.
    pub unlisted: Vec<String>,
}

impl CommandFound {
    /// Reads what a `command` finding holds.
    ///
    /// # Errors
    ///
    /// `input`/`finding_invalid` when the finding is not one of this pack's.
    pub fn read(finding: &Finding) -> Result<Self, Failure> {
        written::from_text(finding.found()).ok_or_else(|| {
            Failure::new(
                FailureClass::Input,
                a2m_core::FailureCode::FINDING_INVALID,
                "A `command` finding does not hold what the command pack's discoverer writes.",
                "Run the discovery again with the version of the pack that is enabled.",
            )
        })
    }
}

/// Reads a finding's text back with the crate's own JSON reader, and
/// accepts only what the discoverer itself would have written.
mod written {
    use a2m_core::PackPayload;

    use super::CommandFound;
    use crate::json::{self, Json};
    use crate::metadata::{OptionFound, OptionSaid, Takes};

    fn text(value: &Json) -> Option<String> {
        match value {
            Json::Text(text) => Some(text.clone()),
            _ => None,
        }
    }

    fn field<'a>(fields: &'a [(String, Json)], name: &str) -> Option<&'a Json> {
        fields.iter().find(|(key, _)| key == name).map(|(_, v)| v)
    }

    fn texts(value: &Json) -> Option<Vec<String>> {
        match value {
            Json::List(items) => items.iter().map(text).collect(),
            _ => None,
        }
    }

    fn maybe(value: &Json) -> Option<Option<String>> {
        match value {
            Json::Null => Some(None),
            other => text(other).map(Some),
        }
    }

    fn option(value: &Json) -> Option<OptionFound> {
        let Json::Object(fields) = value else {
            return None;
        };
        let said = match field(fields, "said")? {
            Json::List(items) => items
                .iter()
                .map(|item| {
                    let Json::Object(said) = item else {
                        return None;
                    };
                    Some(OptionSaid {
                        from: text(field(said, "from")?)?,
                        takes: Takes::from_str(&text(field(said, "takes")?)?)?,
                        value: maybe(field(said, "value")?)?,
                        about: maybe(field(said, "about")?)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
            _ => return None,
        };
        Some(OptionFound {
            flags: texts(field(fields, "flags")?)?,
            said,
            disagree: match field(fields, "disagree")? {
                Json::Bool(disagree) => *disagree,
                _ => return None,
            },
        })
    }

    pub(super) fn from_text(found: &str) -> Option<CommandFound> {
        let Json::Object(fields) = json::parse(found).ok()? else {
            return None;
        };
        let version = match field(&fields, "version")? {
            Json::Null => None,
            Json::Object(version) => Some(super::VersionSeen {
                args: match field(version, "args")? {
                    Json::List(items) => items.iter().map(text).collect::<Option<Vec<_>>>()?,
                    _ => return None,
                },
                pattern: text(field(version, "pattern")?)?,
                version: text(field(version, "version")?)?,
            }),
            _ => return None,
        };
        let read = CommandFound {
            name: text(field(&fields, "name")?)?,
            about: text(field(&fields, "about")?)?,
            usage: match field(&fields, "usage")? {
                Json::Null => None,
                other => Some(text(other)?),
            },
            platform: text(field(&fields, "platform")?)?,
            digest: text(field(&fields, "digest")?)?,
            version,
            options: match field(&fields, "options") {
                None => Vec::new(),
                Some(Json::List(items)) => items.iter().map(option).collect::<Option<Vec<_>>>()?,
                Some(_) => return None,
            },
        };
        // Only what this crate itself would have written is accepted.
        let again = PackPayload::encode(&read).ok()?;
        (again.as_bytes() == found.as_bytes()).then_some(read)
    }
}

/// Whether a word of a command's name is plain: letters, digits, `_.:-`.
fn plain(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}

/// The commands a help text lists: the indented lines under a line that
/// ends with `ommands:`, each a name, two or more spaces, and what it does.
pub(crate) fn listed_commands(help: &str) -> (Vec<(String, String)>, u64) {
    let mut found = Vec::new();
    let mut dropped = 0;
    let mut listing = false;
    for line in help.lines() {
        if line.trim_end().to_ascii_lowercase().ends_with("commands:") && !line.starts_with(' ') {
            listing = true;
            continue;
        }
        if !listing {
            continue;
        }
        if line.trim().is_empty() || !line.starts_with([' ', '\t']) {
            listing = false;
            continue;
        }
        let entry = line.trim();
        let (name, about) = entry
            .split_once("  ")
            .map_or((entry, ""), |(name, about)| (name, about.trim()));
        if !name.is_empty() && name.split(' ').all(plain) {
            found.push((name.to_owned(), about.to_owned()));
        } else {
            dropped += 1;
        }
    }
    (found, dropped)
}

/// What follows a command's words on the usage line of its own help.
pub(crate) fn usage_of(help: &str, command: &str) -> Option<String> {
    let line = help.lines().find_map(|line| {
        let line = line.trim();
        line.get(..6)
            .filter(|start| start.eq_ignore_ascii_case("usage:"))
            .map(|_| line[6..].trim())
    })?;
    // The program's own name, then the command's words.
    let (_, after_program) = line.split_once(' ')?;
    let rest = after_program.trim_start().strip_prefix(command)?;
    (rest.is_empty() || rest.starts_with(' ')).then(|| rest.trim().to_owned())
}

/// The discoverer of the command pack.
pub(crate) struct HelpDiscoverer {
    pub clock: Arc<dyn Clock>,
}

fn probe_step(program: u32, probe: Probe) -> Result<Step, Failure> {
    Ok(Step {
        kind: SPAWN,
        refers_to: None,
        args: Vec::new(),
        timeout_ms: 10_000,
        fails_as: FailsAs {
            class: FailureClass::Transient,
            code: TIMEOUT,
        },
        commits: false,
        // A form the target does not declare is reported absent.
        lookup: true,
        idempotent: false,
        when: None,
        payload: PackPayload::encode(&StepPayload::Probe { program, probe })?,
    })
}

/// Runs one declared form. `None` when the target declares no such form.
async fn run(
    port: &dyn SandboxPort,
    scope: &DiscoveryScope<'_>,
    program: u32,
    probe: Probe,
) -> Result<Option<Values>, Failure> {
    let step = probe_step(program, probe)?;
    let args = Values::new();
    let input = StepInput::new(&args, scope.deadline);
    match port.act(&step, &input).await {
        Ok(StepOutcome::Done(observed)) => Ok(Some(observed.values)),
        Ok(StepOutcome::Absent(_)) => Ok(None),
        Err(failure) => Err(failure.failure),
    }
}

fn text<'a>(values: &'a Values, name: &str) -> &'a str {
    values
        .get(name)
        .and_then(Value::as_text)
        .unwrap_or_default()
}

fn succeeded(values: &Values) -> bool {
    values.get("exit").and_then(Value::as_int) == Some(0)
        && values.get("truncated").and_then(Value::as_bool) == Some(false)
}

/// The name a fish completion line gives the program with `-c`: the file
/// name of the surface's path, without the `.exe` of a Windows program.
fn program_name(address: &str) -> String {
    let file = address.rsplit(['/', '\\']).next().unwrap_or(address);
    match file.len().checked_sub(4) {
        Some(at) if file.is_char_boundary(at) && file[at..].eq_ignore_ascii_case(".exe") => {
            file[..at].to_owned()
        }
        _ => file.to_owned(),
    }
}

fn file_unreadable(name: &str, why: &str) -> Failure {
    codes::failure(
        FailureClass::Input,
        codes::COMMAND_FILE_UNREADABLE,
        format!("The file handed to the scout as `{name}` cannot be read: {why}."),
        "Hand the file as plain UTF-8 text, or a manual page compressed with gzip; uncompress a page compressed any other way first.",
    )
}

fn produced_by() -> ProducedBy {
    ProducedBy {
        provider: PACK.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        component: None,
    }
}

fn finding_failed(_: a2m_core::InvalidFinding) -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_core::FailureCode::FINDING_INVALID,
        "A finding about a listed command could not be made.",
        "Report this to the command pack's author.",
    )
}

fn payload<T: Serialize>(found: &T) -> Result<String, Failure> {
    PackPayload::encode(found)
        .map(|payload| String::from_utf8_lossy(payload.as_bytes()).into_owned())
}

/// Reads each file the scout was handed under a name this discoverer
/// declares, before any program is started: a file that cannot be read
/// stops the discovery with nothing run. The text is redacted whole before
/// it is read.
fn handed(scope: &DiscoveryScope<'_>) -> Result<Vec<(Handed, Digest)>, Failure> {
    let program = program_name(&scope.surface.surface.address);
    let limit = usize::try_from(scope.bounds.bytes).unwrap_or(usize::MAX);
    let mut files = Vec::new();
    for name in [COMPLETIONS, MANUAL] {
        let Some(path) = scope.paths.get(name) else {
            continue;
        };
        let bytes = path.read(scope.bounds.bytes)?;
        let text = if name == MANUAL {
            manual::text_of(&bytes, limit)
        } else {
            String::from_utf8(bytes.clone()).map_err(|_| "it is not UTF-8 text")
        }
        .map_err(|why| file_unreadable(name, why))?;
        let text = scope.redact.redact(&text);
        let read: FileRead = if name == MANUAL {
            manual::read(&text, &program, scope.redact).map_err(|why| file_unreadable(name, why))?
        } else {
            fish::read(&text, &program, scope.redact)
        };
        files.push((Handed { name, read }, Digest::of(&bytes)));
    }
    Ok(files)
}

impl HelpDiscoverer {
    /// The finding about one handed file.
    fn file_finding(
        &self,
        file: &Handed,
        digest: Digest,
        listed: &[String],
    ) -> Result<Finding, Failure> {
        let (program, unlisted) = crate::metadata::beyond_commands(&file.read, listed);
        let unread = file.read.unread_total();
        let found = CommandFileFound {
            name: file.name.to_owned(),
            form: file.read.form.to_owned(),
            read: file.read.read,
            unread: fish::unread_counts(&file.read),
            program_options: program
                .into_iter()
                .map(|option| ProgramOption {
                    flags: option.flags,
                    takes: option.takes,
                    value: option.value,
                    about: option.about,
                })
                .collect(),
            unlisted,
        };
        let format = if file.name == MANUAL {
            "manual_page"
        } else {
            "fish_completion"
        };
        let mut finding = Finding::about(
            Subject::File {
                digest,
                format: format.to_owned(),
                part: None,
            },
            codes::file_finding_kind(),
            payload(&found)?,
            Source::Artifact,
            produced_by(),
            self.clock.now(),
        )
        .level(EvidenceLevel::Inferred)
        .coverage(if unread == 0 {
            Coverage::Complete
        } else {
            Coverage::Partial
        })
        .limitation(
            LimitationCode::NotExercised,
            "The file was read as text and never run. Nothing it says of the program was tried, and a line or an entry it holds in a form the reader does not know is counted as unread, not interpreted.",
        );
        if file.read.unread.contains_key("bound") {
            finding = finding.limitation(
                LimitationCode::Truncated,
                "The file states more options than the reader takes from one file.",
            );
        }
        finding.build().map_err(finding_failed)
    }

    /// The finding about one listed command. With options a handed file
    /// states, it is `inferred` from those files; without, it is what the
    /// help showed, at `discovered`.
    fn command_finding(
        &self,
        found: &CommandFound,
        digest: Digest,
        files: &[(Handed, Finding)],
    ) -> Result<Finding, Failure> {
        let subject = Subject::File {
            digest,
            format: "program".to_owned(),
            part: None,
        };
        if found.options.is_empty() {
            return Finding::about(
                subject,
                codes::finding_kind(),
                payload(found)?,
                Source::LiveSystem,
                produced_by(),
                self.clock.now(),
            )
            .level(EvidenceLevel::Discovered)
            .limitation(
                LimitationCode::NotExercised,
                "The program's help lists the command. It was never run, and what its arguments mean is read from its usage line.",
            )
            .build()
            .map_err(finding_failed);
        }
        let mut finding = Finding::about(
            subject,
            codes::finding_kind(),
            payload(found)?,
            Source::Artifact,
            produced_by(),
            self.clock.now(),
        )
        .level(EvidenceLevel::Inferred)
        .limitation(
            LimitationCode::NotExercised,
            "The program's help lists the command, and the files handed to the scout state options of it, each with its source. Nothing was run, and where two sources disagree neither is chosen.",
        );
        for (file, parent) in files {
            let used = found
                .options
                .iter()
                .any(|option| option.said.iter().any(|said| said.from == file.name));
            if used {
                finding = finding.derived_from(parent);
            }
        }
        finding.build().map_err(finding_failed)
    }

    async fn look(
        &self,
        port: &dyn SandboxPort,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let files = handed(scope)?;
        let mut report = DiscoveryReport::default();
        let mut seen = 0_usize;
        let mut commands: Vec<(CommandFound, Digest)> = Vec::new();
        'programs: for program in 0..MOST_PROGRAMS {
            let version = run(port, scope, program, Probe::Version)
                .await?
                .and_then(|values| {
                    let args = values
                        .get("elements")
                        .and_then(Value::as_list)?
                        .iter()
                        .filter_map(|item| item.as_text().map(str::to_owned))
                        .collect();
                    Some(VersionSeen {
                        args,
                        pattern: values.get("pattern")?.as_text()?.to_owned(),
                        version: scope.redact.redact(values.get("version")?.as_text()?),
                    })
                });
            let mut overview = None;
            for index in 0..MOST_FORMS {
                let probe = Probe::Help {
                    index,
                    command: None,
                };
                if let Some(values) = run(port, scope, program, probe).await?
                    && succeeded(&values)
                {
                    overview = Some(values);
                    break;
                }
            }
            let Some(overview) = overview else {
                continue;
            };
            let Ok(digest) = text(&overview, "digest").parse::<Digest>() else {
                continue;
            };
            // The driver redacted what the program printed; the scope's own
            // redactor sees the whole text too, before any line of it is
            // cut out.
            let help = scope.redact.redact(text(&overview, "stdout"));
            let (listed, dropped) = listed_commands(&help);
            report.dropped += dropped;
            for (name, about) in listed {
                if scope.deadline.passed() {
                    report.bounds_hit.push("seconds".to_owned());
                    break 'programs;
                }
                if seen == MOST_COMMANDS {
                    report.bounds_hit.push("depth".to_owned());
                    break 'programs;
                }
                seen += 1;
                let mut usage = None;
                for index in 0..MOST_FORMS {
                    let probe = Probe::Help {
                        index,
                        command: Some(name.clone()),
                    };
                    if let Some(values) = run(port, scope, program, probe).await?
                        && succeeded(&values)
                    {
                        let own = scope.redact.redact(text(&values, "stdout"));
                        usage = usage_of(&own, &name);
                        break;
                    }
                }
                commands.push((
                    CommandFound {
                        name: scope.redact.redact(&name),
                        about: scope.redact.redact(&about),
                        usage: usage.map(|usage| scope.redact.redact(&usage)),
                        platform: current_platform(),
                        digest: digest.to_string(),
                        version: version.clone(),
                        options: Vec::new(),
                    },
                    digest,
                ));
            }
        }
        // The files' own findings come first: a command's finding that uses
        // what a file says is derived from that file's.
        let listed: Vec<String> = commands.iter().map(|(found, _)| found.name.clone()).collect();
        let mut parents = Vec::new();
        for (file, digest) in files {
            report.dropped += file.read.unread_total();
            let finding = self.file_finding(&file, digest, &listed)?;
            out.found(finding.clone())?;
            parents.push((file, finding));
        }
        let files_read: Vec<&Handed> = parents.iter().map(|(file, _)| file).collect();
        for (mut found, digest) in commands {
            found.options =
                crate::metadata::options_of(&found.name, found.usage.as_deref(), &files_read);
            out.found(self.command_finding(&found, digest, &parents)?)?;
        }
        Ok(report)
    }
}

impl Discoverer for HelpDiscoverer {
    fn name(&self) -> DiscovererName {
        DiscovererName::new(DISCOVERER)
            .unwrap_or_else(|_| unreachable!("`help` is a well-formed discoverer name"))
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == SURFACE_KIND
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.look(port, scope, out))
    }
}

/// Puts `command` findings so that an agent can read them.
pub(crate) struct HelpDescriber {
    /// A line is put together from parts of a finding, and is redacted as
    /// a whole after that: parts that hold no secret can make one.
    pub redact: Arc<dyn Redact>,
}

impl Describer for HelpDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let read: Vec<CommandFound> = findings
            .iter()
            .filter_map(|finding| CommandFound::read(finding).ok())
            .collect();
        let mut untrusted = Vec::new();
        for found in &read {
            let line = match (form, &found.usage) {
                (DescribeForm::Full, Some(usage)) => {
                    format!("{} {} — {}", found.name, usage, found.about)
                }
                _ => found.name.clone(),
            };
            untrusted.push(self.redact.redact(&line));
        }
        // What the handed files gave: how many, how much of them was not
        // read, and how many commands they state options of.
        let files: Vec<&Finding> = findings
            .iter()
            .filter(|finding| *finding.kind() == codes::file_finding_kind())
            .collect();
        let unread: u64 = files
            .iter()
            .filter_map(|finding| match crate::json::parse(finding.found()).ok()? {
                crate::json::Json::Object(fields) => fields
                    .into_iter()
                    .find(|(key, _)| key == "unread")
                    .map(|(_, unread)| unread),
                _ => None,
            })
            .map(|unread| match unread {
                crate::json::Json::Object(counts) => counts
                    .iter()
                    .filter_map(|(_, count)| match count {
                        crate::json::Json::Number(digits) => digits.parse::<u64>().ok(),
                        _ => None,
                    })
                    .sum(),
                _ => 0,
            })
            .sum();
        let described = read.iter().filter(|found| !found.options.is_empty()).count();
        let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
        let mut text = format!(
            "The program's help lists {} commands. None was run: whether a command reads or writes is not known until a person declares it.",
            read.len()
        );
        if !files.is_empty() {
            text.push_str(&format!(
                " {} handed files were read as text and never run; they state options of {described} commands, and {unread} of their lines or entries were in a form the reader does not take and were not read.",
                files.len()
            ));
        }
        Description {
            text,
            data: Values::new()
                .with("commands", count(read.len()))
                .with("files", count(files.len()))
                .with("described", count(described))
                .with("unread", i64::try_from(unread).unwrap_or(i64::MAX)),
            untrusted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{listed_commands, usage_of};

    #[test]
    fn the_listed_commands_and_a_usage_line_are_read_from_help() {
        let help = "example-cli 2.4.1\n\nUsage: example-cli [--home <dir>] <command> [arguments]\n\nCommands:\n  item show      Show one item.\n  item set       Set the quantity of one item.\n  $(reboot)      Not a name.\n\nOptions:\n  --help         Show this\n";
        let (commands, dropped) = listed_commands(help);
        assert_eq!(
            commands,
            [
                ("item show".to_owned(), "Show one item.".to_owned()),
                (
                    "item set".to_owned(),
                    "Set the quantity of one item.".to_owned()
                ),
            ]
        );
        assert_eq!(dropped, 1);
        let own = "Usage: example-cli item show <sku> [--format <form>]\n\nShow one item.\n";
        assert_eq!(
            usage_of(own, "item show").as_deref(),
            Some("<sku> [--format <form>]")
        );
        assert_eq!(usage_of(own, "item set"), None);
        assert_eq!(usage_of("no usage here", "item show"), None);
    }
}
