//! Discovery: what a program says about itself, through the help and
//! version forms its target declares and nothing else.
//!
//! The discoverer starts no command. It reads the list of commands from
//! the program's help, and each listed command's usage line, and makes one
//! finding for each: seen to be listed, never run.

use std::sync::Arc;

use a2m_core::{
    Digest, EvidenceLevel, FailsAs, Failure, FailureClass, Finding, LimitationCode, PackPayload,
    ProducedBy, Redact, Source, Step, Subject, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
    current_platform,
};
use a2m_telemetry::Clock;
use serde::{Deserialize, Serialize};

use crate::codes::{self, DISCOVERER, PACK, SPAWN, SURFACE_KIND, TIMEOUT};
use crate::model::{Probe, StepPayload};

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

    fn text(value: &Json) -> Option<String> {
        match value {
            Json::Text(text) => Some(text.clone()),
            _ => None,
        }
    }

    fn field<'a>(fields: &'a [(String, Json)], name: &str) -> Option<&'a Json> {
        fields.iter().find(|(key, _)| key == name).map(|(_, v)| v)
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
    let input = StepInput {
        args: &args,
        record: None,
        key: None,
        stamp: None,
        deadline: scope.deadline,
    };
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

impl HelpDiscoverer {
    async fn look(
        &self,
        port: &dyn SandboxPort,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let mut report = DiscoveryReport::default();
        let mut seen = 0_usize;
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
            let (commands, dropped) = listed_commands(&help);
            report.dropped += dropped;
            for (name, about) in commands {
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
                let found = CommandFound {
                    name: scope.redact.redact(&name),
                    about: scope.redact.redact(&about),
                    usage: usage.map(|usage| scope.redact.redact(&usage)),
                    platform: current_platform(),
                    digest: digest.to_string(),
                    version: version.clone(),
                };
                let finding = Finding::about(
                    Subject::File {
                        digest,
                        format: "program".to_owned(),
                        part: None,
                    },
                    codes::finding_kind(),
                    PackPayload::encode(&found)
                        .map(|payload| String::from_utf8_lossy(payload.as_bytes()).into_owned())?,
                    Source::LiveSystem,
                    ProducedBy {
                        provider: PACK.to_owned(),
                        version: env!("CARGO_PKG_VERSION").to_owned(),
                        component: None,
                    },
                    self.clock.now(),
                )
                .level(EvidenceLevel::Discovered)
                .limitation(
                    LimitationCode::NotExercised,
                    "The program's help lists the command. It was never run, and what its arguments mean is read from its usage line.",
                )
                .build()
                .map_err(|_| {
                    Failure::new(
                        FailureClass::Input,
                        a2m_core::FailureCode::FINDING_INVALID,
                        "A finding about a listed command could not be made.",
                        "Report this to the command pack's author.",
                    )
                })?;
                out.found(finding)?;
            }
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
        Description {
            text: format!(
                "The program's help lists {} commands. None was run: whether a command reads or writes is not known until a person declares it.",
                read.len()
            ),
            data: Values::new().with("commands", i64::try_from(read.len()).unwrap_or(i64::MAX)),
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
