//! Discovery on a device link: the device is asked to identify itself, and
//! nothing else.
//!
//! A read is not always without effect on a device, so nothing here reads
//! a register and nothing scans addresses. The first protocol offers no
//! listing of a device's own objects, so identification is all there is to
//! observe. The drafting rule turns what the device reported into the start
//! of a map: its model, its firmware version, and no register.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{
    Coverage, EvidenceLevel, FailsAs, Failure, FailureClass, FailureCode, FileKindName, Finding,
    FindingKind, PackPayload, ProducedBy, Source, Step, Subject, Value, Values,
};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::Clock;

use crate::codes;
use crate::lower::StepPayload;
use crate::map::IDENTITY_FIELDS;
use crate::names::{Names, PACK};

/// The discoverer's name, as the manifest declares it.
pub(crate) const DISCOVERER: &str = "device_identity";
/// How long the device may take to say what it is.
const IDENTIFY_MS: u32 = 5_000;

/// What a `device_identity` finding holds in `found`: what the device
/// reported, as JSON with its keys in order. Every key is one of
/// [`IDENTITY_FIELDS`], and a key the device did not report is absent.
pub(crate) type Reported = BTreeMap<String, String>;

fn unreadable(what: &str) -> Failure {
    codes::failure(
        FailureClass::Input,
        FailureCode::FINDING_INVALID,
        format!("A `device_identity` finding cannot be read: {what}."),
        "Run discovery on the device link again.",
    )
}

fn reported(finding: &Finding) -> Result<Reported, Failure> {
    let reported: Reported = serde_json::from_str(finding.found())
        .map_err(|_| unreadable("it does not hold what a device reported"))?;
    if reported.keys().any(|key| !IDENTITY_FIELDS.contains(&key.as_str())) {
        return Err(unreadable("it holds a word no device is asked for"));
    }
    Ok(reported)
}

/// Asks the device on a link what it is.
pub(crate) struct IdentityDiscoverer {
    names: Arc<Names>,
    name: DiscovererName,
    clock: Arc<dyn Clock>,
    version: String,
}

impl IdentityDiscoverer {
    pub fn new(names: Arc<Names>, clock: Arc<dyn Clock>, version: String) -> Result<Self, Failure> {
        Ok(Self {
            names,
            name: DiscovererName::new(DISCOVERER)?,
            clock,
            version,
        })
    }
}

impl Discoverer for IdentityDiscoverer {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == self.names.surface_kind
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            // The one step discovery sends: the request to identify. It
            // commits nothing, and the port would refuse it if it did.
            let step = Step {
                kind: self.names.open.clone(),
                refers_to: None,
                args: Vec::new(),
                timeout_ms: IDENTIFY_MS,
                fails_as: FailsAs {
                    class: FailureClass::UnknownVariant,
                    code: codes::UNEXPECTED_REPLY,
                },
                commits: false,
                lookup: false,
                idempotent: false,
                when: None,
                payload: PackPayload::encode(&StepPayload::Report)?,
            };
            let args = Values::new();
            let input = StepInput {
                args: &args,
                record: None,
                key: None,
                stamp: None,
                deadline: scope.deadline,
            };
            let observed = match port.act(&step, &input).await {
                Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) => observed,
                Err(failure) => return Err(failure.failure),
            };
            let reported: Reported = IDENTITY_FIELDS
                .iter()
                .filter_map(|field| {
                    let text = observed.values.get(field).and_then(Value::as_text)?;
                    Some(((*field).to_owned(), scope.redact.redact(text)))
                })
                .collect();
            let (Some(model), Some(firmware)) =
                (reported.get("model"), reported.get("firmware_version"))
            else {
                return Err(codes::failure(
                    FailureClass::UnknownVariant,
                    codes::DEVICE_IDENTITY_MISMATCH,
                    "The device does not report its model and its firmware version, so it cannot be the subject of a finding.",
                    "Check that the surface names the protocol the device speaks.",
                ));
            };
            let subject = Subject::Device {
                reported_id: reported.get("serial").cloned().unwrap_or_default(),
                model: model.clone(),
                firmware: firmware.clone(),
            };
            let found = serde_json::to_string(&reported)
                .map_err(|_| unreadable("what the device reported cannot be written"))?;
            let finding = Finding::about(
                subject,
                self.names.identity_finding.clone(),
                found,
                Source::LiveSystem,
                ProducedBy {
                    provider: PACK.to_owned(),
                    version: self.version.clone(),
                    component: None,
                },
                self.clock.now(),
            )
            .level(EvidenceLevel::Discovered)
            // The device was asked what it is, and that is all it was asked.
            .coverage(Coverage::Partial)
            .build()?;
            out.found(finding)?;
            Ok(DiscoveryReport {
                coverage: Coverage::Partial,
                bounds_hit: Vec::new(),
                dropped: 0,
            })
        })
    }
}

/// Puts identity findings so that an agent can read them.
pub(crate) struct IdentityDescriber;

impl Describer for IdentityDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let devices: Vec<Reported> = findings
            .iter()
            .filter_map(|finding| reported(finding).ok())
            .collect();
        let count = i64::try_from(devices.len()).unwrap_or(i64::MAX);
        let mut description = Description {
            text: format!(
                "{count} device(s) identified themselves. No register was read: a map of registers comes from the device's documentation."
            ),
            data: Values::new().with("devices", count),
            untrusted: Vec::new(),
        };
        // What a device says about itself is the device's text, never ours.
        for device in &devices {
            match form {
                DescribeForm::Summary => description
                    .untrusted
                    .extend(device.get("model").cloned()),
                DescribeForm::Full => description.untrusted.extend(
                    device
                        .iter()
                        .map(|(field, text)| format!("{field}: {text}")),
                ),
            }
        }
        description
    }
}

/// A name made from a device's own word for its model: lowercase letters,
/// digits, and underscores, starting with a letter.
fn map_name(model: &str) -> Option<String> {
    let mut name = String::new();
    for character in model.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            name.push(character);
        } else if !name.ends_with('_') && !name.is_empty() {
            name.push('_');
        }
    }
    let name = name.trim_end_matches('_').to_owned();
    (name.starts_with(|first: char| first.is_ascii_lowercase()) && name.len() <= 48)
        .then_some(name)
}

/// From an identity finding to the start of a map.
pub(crate) struct MapFromIdentity {
    names: Arc<Names>,
}

impl MapFromIdentity {
    pub const fn new(names: Arc<Names>) -> Self {
        Self { names }
    }
}

impl DraftingRule for MapFromIdentity {
    fn finding_kind(&self) -> FindingKind {
        self.names.identity_finding.clone()
    }

    fn file_kind(&self) -> FileKindName {
        self.names.register_kind.clone()
    }

    /// Drafts a map with the model and the firmware version the device
    /// reported, and no register: the device listed none. It writes no
    /// range, no hazard, no safe state, and no unit.
    fn draft(&self, finding: &Finding, _cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let reported = reported(finding)?;
        let (Some(model), Some(firmware)) =
            (reported.get("model"), reported.get("firmware_version"))
        else {
            return Err(unreadable("it states no model or no firmware version"));
        };
        let name = map_name(model)
            .ok_or_else(|| unreadable("the model it states gives no name for a map"))?;
        let reads = reported
            .keys()
            .map(|field| DraftValue::text(field))
            .collect();
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("registers/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("device_map", DraftValue::text(&name)),
                ("model", DraftValue::text(model)),
                ("firmware", DraftValue::List(vec![DraftValue::text(firmware)])),
                (
                    "identify",
                    DraftValue::map(vec![("reads", DraftValue::List(reads))]),
                ),
                ("registers", DraftValue::Map(Vec::new())),
            ]),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::map_name;

    #[test]
    fn a_map_is_named_from_the_models_own_word() {
        assert_eq!(
            map_name("example-controller").as_deref(),
            Some("example_controller")
        );
        assert_eq!(map_name("Example  Pump/2").as_deref(), Some("example_pump_2"));
        assert_eq!(map_name("2000"), None);
        assert_eq!(map_name("--"), None);
    }
}
