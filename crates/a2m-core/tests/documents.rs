//! Holds the names in the code and the names in the design documents
//! together. If a document gains, loses, renames, or reorders a value and the
//! code does not follow, or the other way round, a test here fails.

use std::path::Path;

use a2m_core::{
    Archetype, BindingStatus, Coverage, EffectClass, EvidenceLevel, FailureClass, Hazard,
    LimitationCode, RecordState, RunState, SessionState, Source, Subject,
};

/// Reads a document from the repository's `docs` folder.
fn document(path: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The rows of the first table in `text` whose header line starts with
/// `header`, as lists of trimmed cells.
fn table(text: &str, header: &str) -> Vec<Vec<String>> {
    let rows: Vec<Vec<String>> = text
        .lines()
        .skip_while(|line| !line.starts_with(header))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect();
    assert!(!rows.is_empty(), "no table starting with {header:?}");
    rows
}

/// One column of a table, with the code marks removed.
fn column(text: &str, header: &str, index: usize) -> Vec<String> {
    table(text, header)
        .into_iter()
        .map(|row| row[index].trim_matches('`').to_owned())
        .collect()
}

/// The part of `text` from the line `start` up to the line `end`.
fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text.find(start).unwrap_or_else(|| panic!("no {start:?}"));
    let to = text[from..]
        .find(end)
        .unwrap_or_else(|| panic!("no {end:?} after {start:?}"));
    &text[from..from + to]
}

/// Every name written as code in `text`, in order.
fn code_words(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn failure_classes_are_the_ones_in_the_technical_reference() {
    let reference = document("reference/technical-reference.md");
    let section = between(&reference, "## Failure codes", "## Command line");
    assert_eq!(column(section, "| Class |", 0), FailureClass::NAMES);
}

#[test]
fn failure_classes_are_the_ones_the_engine_section_responds_to() {
    let mut in_spec = column(
        &document("spec/08-execution-engine.md"),
        "| Class | Example | Response |",
        0,
    );
    let mut in_code: Vec<String> = FailureClass::NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    in_spec.sort();
    in_code.sort();
    assert_eq!(in_spec, in_code);
}

#[test]
fn run_states_are_the_ones_in_the_ledger_design() {
    let spec = document("spec/42-run-ledger-design.md");
    let section = between(&spec, "## Run states", "## Record states");
    assert_eq!(column(section, "| State |", 0), RunState::NAMES);
}

#[test]
fn final_run_states_are_the_ones_the_ledger_design_marks_final() {
    let spec = document("spec/42-run-ledger-design.md");
    let section = between(&spec, "## Run states", "## Record states");
    let marked: Vec<String> = table(section, "| State |")
        .into_iter()
        .filter(|row| row[2] == "Final")
        .map(|row| row[0].trim_matches('`').to_owned())
        .collect();
    let in_code: Vec<&str> = RunState::ALL
        .iter()
        .filter(|state| state.is_final())
        .map(|state| state.as_str())
        .collect();
    assert_eq!(marked, in_code);
}

#[test]
fn record_states_are_the_ones_in_the_ledger_design() {
    let spec = document("spec/42-run-ledger-design.md");
    let section = between(&spec, "## Record states", "## Rules that must");
    assert_eq!(column(section, "| State |", 0), RecordState::NAMES);
}

#[test]
fn session_states_are_the_ones_in_the_session_keeper() {
    let spec = document("spec/44-session-keeper.md");
    let section = between(&spec, "## States", "## Three clocks");
    assert_eq!(column(section, "| State |", 0), SessionState::NAMES);
}

#[test]
fn effect_classes_are_the_ones_in_the_safety_section() {
    let spec = document("spec/18-safety-policy-and-approvals.md");
    assert_eq!(column(&spec, "| Effect |", 0), EffectClass::NAMES);
}

#[test]
fn hazards_are_the_ones_in_the_effects_section() {
    let spec = document("spec/61-effects-hazards-and-consent.md");
    assert_eq!(column(&spec, "| Hazard |", 0), Hazard::NAMES);
}

#[test]
fn evidence_levels_are_the_ladder_in_its_section() {
    let spec = document("spec/59-the-evidence-ladder.md");
    assert_eq!(column(&spec, "| Level |", 0), EvidenceLevel::NAMES);
}

#[test]
fn archetypes_are_the_seven_in_their_section() {
    let spec = document("spec/54-interaction-archetypes.md");
    let section = between(
        &spec,
        "## The seven",
        "## How each archetype fills the loop",
    );
    let named: Vec<String> = column(section, "| # | Archetype |", 1)
        .into_iter()
        .map(|name| name.to_lowercase())
        .collect();
    assert_eq!(named, Archetype::NAMES);
}

#[test]
fn binding_statuses_are_the_ones_in_the_file_formats() {
    let formats = document("reference/file-formats.md");
    let line = formats
        .lines()
        .find(|line| line.starts_with("**Binding.**"))
        .expect("the Binding paragraph");
    let listed = between(line, "`status` (", ")");
    assert_eq!(code_words(listed)[1..], *BindingStatus::NAMES);
}

#[test]
fn coverage_values_are_the_ones_a_finding_may_carry() {
    let spec = document("spec/58-discovery-and-findings.md");
    let row = table(&spec, "| Field | Meaning |")
        .into_iter()
        .find(|row| row[0] == "`coverage`")
        .expect("the coverage row");
    assert_eq!(code_words(&row[1]), Coverage::NAMES);
}

#[test]
fn finding_sources_are_the_ones_in_the_discovery_section() {
    let spec = document("spec/58-discovery-and-findings.md");
    assert_eq!(column(&spec, "| Source |", 0), Source::NAMES);
}

#[test]
fn each_source_supports_the_level_the_discovery_section_gives() {
    let spec = document("spec/58-discovery-and-findings.md");
    let in_spec: Vec<String> = table(&spec, "| Source |")
        .into_iter()
        .map(|row| row[2].trim_matches('`').to_owned())
        .collect();
    let in_code: Vec<&str> = Source::ALL
        .iter()
        .map(|source| source.highest_level().as_str())
        .collect();
    assert_eq!(in_spec, in_code);
}

#[test]
fn limitation_codes_are_the_ones_in_the_discovery_section() {
    let spec = document("spec/58-discovery-and-findings.md");
    assert_eq!(column(&spec, "| Code |", 0), LimitationCode::NAMES);
}

#[test]
fn kinds_of_subject_are_the_ones_in_the_discovery_section() {
    let spec = document("spec/58-discovery-and-findings.md");
    assert_eq!(column(&spec, "| Tag |", 0), Subject::TAGS);
}

#[test]
fn every_vocabulary_reads_back_what_it_writes() {
    fn round_trip<T>(all: &[T], names: &[&str])
    where
        T: Copy
            + PartialEq
            + std::fmt::Debug
            + std::str::FromStr
            + serde::Serialize
            + serde::de::DeserializeOwned,
        T::Err: std::fmt::Debug,
    {
        assert_eq!(all.len(), names.len());
        for (value, name) in all.iter().zip(names) {
            assert_eq!(name.parse::<T>().unwrap(), *value);
            let json = serde_json::to_string(value).unwrap();
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<T>(&json).unwrap(), *value);
        }
        assert!("no such value".parse::<T>().is_err());
        assert!(serde_json::from_str::<T>("\"no such value\"").is_err());
    }
    round_trip(Archetype::ALL, Archetype::NAMES);
    round_trip(BindingStatus::ALL, BindingStatus::NAMES);
    round_trip(Coverage::ALL, Coverage::NAMES);
    round_trip(EffectClass::ALL, EffectClass::NAMES);
    round_trip(EvidenceLevel::ALL, EvidenceLevel::NAMES);
    round_trip(FailureClass::ALL, FailureClass::NAMES);
    round_trip(Hazard::ALL, Hazard::NAMES);
    round_trip(LimitationCode::ALL, LimitationCode::NAMES);
    round_trip(Source::ALL, Source::NAMES);
    round_trip(RecordState::ALL, RecordState::NAMES);
    round_trip(RunState::ALL, RunState::NAMES);
    round_trip(SessionState::ALL, SessionState::NAMES);
}
