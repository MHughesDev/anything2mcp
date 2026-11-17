//! The describer: what was found, put so that an agent can read it. Text
//! the system supplied is kept apart and never mixed into the
//! description.

use std::collections::BTreeMap;

use a2m_core::{Finding, Subject, Value, Values};
use a2m_provider::{DescribeForm, Describer, Description};
use serde_json::Value as Json;

use crate::codes::shown;

/// `web_describer`: findings counted by kind, and, in the full form, one
/// line of this crate's own words for each.
pub(crate) struct WebDescriber;

impl Describer for WebDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
        let mut lines = Vec::new();
        let mut untrusted = Vec::new();
        for finding in findings {
            *counts.entry(finding.kind().as_str()).or_default() += 1;
            let found: Json = serde_json::from_str(finding.found()).unwrap_or(Json::Null);
            let word = |name: &str| found.get(name).and_then(Json::as_str).unwrap_or_default();
            let mut line = Values::new()
                .with("kind", finding.kind().as_str())
                .with("level", finding.level().to_string())
                .with("coverage", finding.coverage().to_string())
                .with(
                    "limitations",
                    i64::try_from(finding.limitations().len()).unwrap_or(i64::MAX),
                );
            // A path, a title, and a method are the system's own text.
            let path = match finding.subject() {
                Subject::Address(address) => address.path(),
                _ => "",
            };
            match finding.kind().as_str() {
                "screen" => {
                    let elements = found
                        .get("elements")
                        .and_then(Json::as_array)
                        .map_or(0, Vec::len);
                    line.insert("elements", i64::try_from(elements).unwrap_or(i64::MAX));
                    untrusted.push(format!("screen {} {}", shown(path), shown(word("title"))));
                }
                "exchange" | "endpoint" => {
                    line.insert(
                        "answered",
                        found
                            .get("answered")
                            .and_then(Json::as_bool)
                            .unwrap_or(false),
                    );
                    untrusted.push(format!(
                        "{} {} {}",
                        finding.kind().as_str(),
                        shown(word("method")),
                        shown(word("path"))
                    ));
                }
                _ => untrusted.push(format!("{} {}", finding.kind().as_str(), shown(path))),
            }
            lines.push(Value::from(line));
        }
        let listed: Vec<String> = counts
            .iter()
            .map(|(kind, count)| format!("{count} {kind}"))
            .collect();
        let text = format!(
            "{} findings: {}. Nothing here is above `discovered`. Paths, titles, and names are the system's own text and are listed apart.",
            findings.len(),
            if listed.is_empty() {
                "none".to_owned()
            } else {
                listed.join(", ")
            }
        );
        let mut data = Values::new();
        match form {
            DescribeForm::Summary => {
                for (kind, count) in counts {
                    data.insert(kind, count);
                }
            }
            DescribeForm::Full => {
                data.insert("findings", lines);
            }
        }
        Description {
            text,
            data,
            untrusted,
        }
    }
}
