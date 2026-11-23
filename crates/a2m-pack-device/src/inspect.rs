//! Firmware inspection: an optional provider that reads a firmware image
//! the person supplies as a file, and says what it holds (spec section 310,
//! Firmware).
//!
//! It is discovery only. It yields findings and never an operation. The
//! subject of every finding is the image, by digest. What it finds is
//! `inferred`: a setting named in an image is a claim about a device, not
//! an observation of one, and the source is `artifact`, a file read without
//! running it. It lists regions and settings. It decrypts nothing and
//! recovers no keys, and the image is never changed. An image that looks
//! encrypted is a named stop, `edge_blocked`, `image_protected`: it is not
//! read further, and nothing is done to get past it.
//!
//! The provider is off by default. [`crate::add_to`] does not register it:
//! a product that offers it calls [`crate::add_firmware_inspection_to`]
//! after a person switched it on. It sends no frame, and it reads no file
//! but the one handed to it under its declared name.

use std::sync::Arc;

use a2m_core::{
    Coverage, Digest, EvidenceLevel, Failure, Finding, FindingKind, ProducedBy, Source, Subject,
    Values,
};
use a2m_files::names_a_secret;
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, Pack, PackContext, Registrar, SandboxPort, SurfaceDescription,
};
use a2m_telemetry::Clock;
use serde::{Deserialize, Serialize};

use crate::firmware;
use crate::names::Names;

/// The optional provider's own name, as its manifest states it.
pub const PROVIDER: &str = "device_firmware";

/// The manifest of the optional provider.
pub const MANIFEST: &str = include_str!("../anything2mcp-firmware-provider.toml");

/// The discoverer's name, and the kind of finding it makes.
pub(crate) const IMAGE: &str = "firmware_image";

/// The name, among a discovery's checked paths, of an image given as a file:
/// the one name the manifest's `firmware_image` declares under `files`. No
/// other path is read.
pub const FIRMWARE_IMAGE_FILE: &str = "firmware_image";

/// The largest image read, in bytes. A device's image is small; a larger
/// file is not one this reads.
const MOST_IMAGE_BYTES: u64 = 1_048_576;
/// The most settings one image's payload gives.
const MOST_SETTINGS: usize = 64;
/// The longest a setting's key or value may be.
const LONGEST_KEY: usize = 64;
const LONGEST_VALUE: usize = 128;
/// A file at least this long, of a form this does not read, and with this
/// many bits of entropy a byte or more, is taken to be encrypted. It is a
/// heuristic that names a stop; nothing is decrypted either way.
const PROTECTED_AT_LEAST_BYTES: usize = 256;
const PROTECTED_BITS_A_BYTE: f64 = 7.2;

/// The failure code the optional provider produces, in its own table.
pub mod codes {
    a2m_core::failure_codes! {
        IMAGE_PROTECTED = EdgeBlocked, "image_protected",
            meaning "The firmware image looks encrypted or protected, and is not read further.",
            next "Supply the image in the form the maker publishes for inspection, or inspect the device through its own interface; nothing here decrypts an image or recovers a key.";
    }
}

use codes::row;

/// One region of an image: where it lies in the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Region {
    pub name: String,
    pub offset: usize,
    pub length: usize,
}

/// One setting an image's payload states, by its line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Setting {
    pub line: usize,
    pub key: String,
    /// `None` where the key names a secret: its value is withheld, so that
    /// a secret is never in a finding.
    pub value: Option<String>,
}

/// What a `firmware_image` finding holds in `found`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Inspected {
    pub form: String,
    pub bytes: usize,
    pub version: String,
    pub model_code: u16,
    pub checksum_holds: bool,
    pub regions: Vec<Region>,
    pub settings: Vec<Setting>,
    /// Payload lines that are not settings, and settings past the most.
    pub lines_left_out: usize,
}

/// Bits of entropy a byte: 8 for random bytes, far less for text and code.
fn entropy(bytes: &[u8]) -> f64 {
    let mut counts = [0_usize; 256];
    for byte in bytes {
        counts[usize::from(*byte)] += 1;
    }
    let total = bytes.len() as f64;
    counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let share = *count as f64 / total;
            -share * share.log2()
        })
        .sum()
}

/// Whether bytes of a form this does not read look encrypted.
fn looks_protected(bytes: &[u8]) -> bool {
    bytes.len() >= PROTECTED_AT_LEAST_BYTES && entropy(bytes) >= PROTECTED_BITS_A_BYTE
}

/// Whether a setting's value is left out of a finding: the compiler's own
/// test for a secret's name, and a wider one, because a value an image
/// states is hostile text a person did not choose. A secret is never in a
/// finding.
fn withheld(key: &str) -> bool {
    names_a_secret(key)
        || ["token", "credential", "pass", "key", "secret", "auth"]
            .iter()
            .any(|word| key.contains(word))
}

/// What an image holds, from its form and its payload's `key=value` lines.
/// The image is read and never changed.
pub(crate) fn inspect(bytes: &[u8], redact: &dyn a2m_core::Redact) -> Option<Inspected> {
    let form = firmware::read(bytes).ok()?;
    let regions = vec![
        Region {
            name: "head".to_owned(),
            offset: 0,
            length: firmware::HEAD_BYTES,
        },
        Region {
            name: "payload".to_owned(),
            offset: form.payload.start,
            length: form.payload.len(),
        },
        Region {
            name: "tail".to_owned(),
            offset: form.payload.end,
            length: firmware::TAIL_BYTES,
        },
    ];
    let mut settings = Vec::new();
    let mut left_out = 0;
    if let Ok(text) = std::str::from_utf8(&bytes[form.payload.clone()]) {
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let setting = line.split_once('=').and_then(|(key, value)| {
                let (key, value) = (key.trim(), value.trim());
                let key_ok = !key.is_empty()
                    && key.len() <= LONGEST_KEY
                    && key.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.".contains(&byte)
                    });
                let value_ok = value.len() <= LONGEST_VALUE
                    && value.bytes().all(|byte| (0x20..0x7F).contains(&byte));
                (key_ok && value_ok).then(|| (key.to_owned(), value.to_owned()))
            });
            match setting {
                Some((key, value)) if settings.len() < MOST_SETTINGS => {
                    let value = (!withheld(&key)).then(|| redact.redact(&value));
                    settings.push(Setting {
                        line: index + 1,
                        key,
                        value,
                    });
                }
                _ => left_out += 1,
            }
        }
    } else {
        left_out += 1;
    }
    Some(Inspected {
        form: "exfw".to_owned(),
        bytes: bytes.len(),
        version: form.version.clone(),
        model_code: form.model_code,
        checksum_holds: form.checksum_holds(),
        regions,
        settings,
        lines_left_out: left_out,
    })
}

fn unreadable(what: &str) -> Failure {
    failure_row(
        &a2m_core::codes::row::FINDING_INVALID,
        format!("A `{IMAGE}` finding cannot be made or read: {what}."),
        "Run discovery again with the image.",
    )
}

fn failure_row(
    row: &a2m_core::CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    row.failure(message, next)
}

/// Reads an image given as a file.
pub(crate) struct ImageDiscoverer {
    names: Arc<Names>,
    name: DiscovererName,
    kind: FindingKind,
    clock: Arc<dyn Clock>,
    version: String,
}

impl ImageDiscoverer {
    pub fn new(names: Arc<Names>, clock: Arc<dyn Clock>, version: String) -> Result<Self, Failure> {
        Ok(Self {
            names,
            name: DiscovererName::new(IMAGE)?,
            kind: FindingKind::new(IMAGE)?,
            clock,
            version,
        })
    }
}

impl Discoverer for ImageDiscoverer {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == self.names.surface_kind
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            // At most one file: the one handed under the declared name.
            for path in scope.paths.get(FIRMWARE_IMAGE_FILE).into_iter() {
                if scope.deadline.passed() {
                    report.coverage = Coverage::Partial;
                    report.bounds_hit.push("seconds".to_owned());
                    break;
                }
                let reading = path.clone();
                let most = scope.bounds.bytes.min(MOST_IMAGE_BYTES);
                let bytes = tokio::task::spawn_blocking(move || {
                    (reading.entry_kind().ok()? == a2m_provider::EntryKind::File)
                        .then(|| reading.read(most).ok())
                        .flatten()
                })
                .await
                .ok()
                .flatten();
                let Some(bytes) = bytes else {
                    report.dropped += 1;
                    continue;
                };
                let Some(inspected) = inspect(&bytes, scope.redact) else {
                    // An image of a form this does not read. One that looks
                    // encrypted is a named stop; anything else is only not
                    // read.
                    if looks_protected(&bytes) {
                        return Err(failure_row(
                            &row::IMAGE_PROTECTED,
                            "The image looks encrypted or protected, so it was not read. Nothing was decrypted, and no key was looked for.",
                            "Supply the image in the form the maker publishes for inspection, or inspect the device through its own interface.",
                        ));
                    }
                    report.dropped += 1;
                    continue;
                };
                let found = serde_json::to_string(&inspected)
                    .map_err(|_| unreadable("what the image holds cannot be written"))?;
                let finding = Finding::about(
                    Subject::File {
                        digest: Digest::of(&bytes),
                        format: "firmware_image".to_owned(),
                        part: None,
                    },
                    self.kind.clone(),
                    found,
                    Source::Artifact,
                    ProducedBy {
                        provider: PROVIDER.to_owned(),
                        version: self.version.clone(),
                        component: None,
                    },
                    self.clock.now(),
                )
                .level(EvidenceLevel::Inferred)
                .coverage(Coverage::Complete)
                .build()?;
                out.found(finding)?;
            }
            Ok(report)
        })
    }
}

/// Puts what images hold so that an agent can read it. The words an image
/// gives, its settings' keys and values, are kept apart as the image's.
pub(crate) struct ImageDescriber;

impl Describer for ImageDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let images: Vec<Inspected> = findings
            .iter()
            .filter(|finding| finding.kind().as_str() == IMAGE)
            .filter_map(|finding| serde_json::from_str(finding.found()).ok())
            .collect();
        let mut text = format!(
            "{} firmware image(s) read from files; nothing was sent to any device, decrypted, or recovered, and what they hold is inferred, not observed.",
            images.len()
        );
        let mut untrusted = Vec::new();
        let mut settings = 0_i64;
        for (number, image) in images.iter().enumerate() {
            let withheld = image
                .settings
                .iter()
                .filter(|setting| setting.value.is_none())
                .count();
            text.push_str(&format!(
                " Image {}: version {}, {} bytes, {} regions, {} settings ({withheld} withheld because their names are a secret's){}.",
                number + 1,
                image.version,
                image.bytes,
                image.regions.len(),
                image.settings.len(),
                if image.checksum_holds {
                    ""
                } else {
                    ", and its checksum does not hold"
                },
            ));
            settings += i64::try_from(image.settings.len()).unwrap_or(i64::MAX);
            if form == DescribeForm::Full {
                untrusted.extend(image.settings.iter().filter_map(|setting| {
                    setting
                        .value
                        .as_ref()
                        .map(|value| format!("line {}: {}={value}", setting.line, setting.key))
                }));
            }
        }
        Description {
            text,
            data: Values::new()
                .with("images", i64::try_from(images.len()).unwrap_or(i64::MAX))
                .with("settings", settings),
            untrusted,
        }
    }
}

/// The optional provider: firmware inspection, by a file.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirmwareInspectionPack;

impl Pack for FirmwareInspectionPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let names = Arc::new(Names::new()?);
        let version = a2m_provider::Manifest::parse(MANIFEST)?
            .provider
            .version
            .to_string();
        to.discoverer(
            Arc::new(ImageDiscoverer::new(
                names,
                cx.telemetry().clock(),
                version,
            )?),
            Arc::new(ImageDescriber),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(firmware::MAGIC);
        bytes.extend(0x4543_u16.to_be_bytes());
        for number in [2_u16, 5, 0] {
            bytes.extend(number.to_be_bytes());
        }
        bytes.extend(u16::try_from(payload.len()).unwrap().to_be_bytes());
        bytes.extend(payload);
        bytes.extend(firmware::crc32(&bytes).to_be_bytes());
        bytes
    }

    #[test]
    fn regions_and_settings_are_listed_and_a_secret_named_value_is_withheld() {
        let bytes = image(
            b"mode=normal\n# a note\nrate_limit = 10\napi_token=hunter2\nnot a setting\nUPPER=1\n",
        );
        let found = inspect(&bytes, &a2m_core::NoSecrets).unwrap();
        assert_eq!(found.version, "2.5.0");
        assert_eq!(
            found
                .regions
                .iter()
                .map(|region| (region.name.as_str(), region.offset, region.length))
                .collect::<Vec<_>>(),
            [("head", 0, 14), ("payload", 14, 77), ("tail", 91, 4)]
        );
        let keys: Vec<_> = found
            .settings
            .iter()
            .map(|setting| (setting.line, setting.key.as_str(), setting.value.as_deref()))
            .collect();
        assert_eq!(
            keys,
            [
                (1, "mode", Some("normal")),
                (3, "rate_limit", Some("10")),
                (4, "api_token", None),
            ]
        );
        assert_eq!(found.lines_left_out, 2);
        assert!(found.checksum_holds);
        assert!(!serde_json::to_string(&found).unwrap().contains("hunter2"));
    }

    #[test]
    fn bytes_of_another_form_are_not_read_and_random_looking_bytes_look_protected() {
        assert!(inspect(b"plain text, not an image", &a2m_core::NoSecrets).is_none());
        let random: Vec<u8> = (0..1024_u32)
            .map(|n| u8::try_from(n.wrapping_mul(2_654_435_761) >> 13 & 0xFF).unwrap())
            .collect();
        assert!(looks_protected(&random));
        assert!(!looks_protected(&vec![b'a'; 1024]));
        assert!(!looks_protected(b"short"));
    }
}
