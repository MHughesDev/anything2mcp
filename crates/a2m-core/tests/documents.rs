//! Holds the names in the code and the names in the design documents
//! together. If a document gains, loses, renames, or reorders a value and the
//! code does not follow, or the other way round, a test here fails.

use std::path::Path;

use a2m_core::{
    Archetype, Availability, BindingStatus, Checkpoint, Coverage, DoctorState, EffectClass,
    EnvironmentKind, EvidenceLevel, FailureClass, FieldBase, Hazard, IdentityStrategy,
    LimitationCode, RecordState, RunState, Sensitivity, SessionState, Source, Subject,
    SubsystemMove, SubsystemState, TestKind,
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
        &document("spec/2-kernel/207-the-engine-loop.md"),
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
    let spec = document("spec/2-kernel/208-runs-the-ledger-and-resume.md");
    let section = between(&spec, "## Run states", "## Record states");
    assert_eq!(column(section, "| State |", 0), RunState::NAMES);
}

#[test]
fn final_run_states_are_the_ones_the_ledger_design_marks_final() {
    let spec = document("spec/2-kernel/208-runs-the-ledger-and-resume.md");
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
    let spec = document("spec/2-kernel/208-runs-the-ledger-and-resume.md");
    let section = between(&spec, "## Record states", "## Rules that must");
    assert_eq!(column(section, "| State |", 0), RecordState::NAMES);
}

#[test]
fn session_states_are_the_ones_in_the_session_keeper() {
    let spec = document("spec/2-kernel/215-sessions-and-the-session-keeper.md");
    let section = between(&spec, "## States", "## Three clocks");
    assert_eq!(column(section, "| State |", 0), SessionState::NAMES);
}

#[test]
fn effect_classes_are_the_ones_in_the_safety_section() {
    let spec = document("spec/2-kernel/212-policy-and-approvals.md");
    assert_eq!(column(&spec, "| Effect |", 0), EffectClass::NAMES);
}

#[test]
fn hazards_are_the_ones_in_the_effects_section() {
    let spec = document("spec/2-kernel/213-effects-hazards-and-consent.md");
    assert_eq!(column(&spec, "| Hazard |", 0), Hazard::NAMES);
}

#[test]
fn evidence_levels_are_the_ladder_in_its_section() {
    let spec = document("spec/2-kernel/219-the-evidence-ladder.md");
    assert_eq!(column(&spec, "| Level |", 0), EvidenceLevel::NAMES);
}

#[test]
fn archetypes_are_the_seven_in_their_section() {
    let spec = document("spec/2-kernel/201-archetypes-and-modes.md");
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
    let spec = document("spec/2-kernel/218-discovery-and-findings.md");
    let row = table(&spec, "| Field | Meaning |")
        .into_iter()
        .find(|row| row[0] == "`coverage`")
        .expect("the coverage row");
    assert_eq!(code_words(&row[1]), Coverage::NAMES);
}

#[test]
fn finding_sources_are_the_ones_in_the_discovery_section() {
    let spec = document("spec/2-kernel/218-discovery-and-findings.md");
    assert_eq!(column(&spec, "| Source |", 0), Source::NAMES);
}

#[test]
fn each_source_supports_the_level_the_discovery_section_gives() {
    let spec = document("spec/2-kernel/218-discovery-and-findings.md");
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
    let spec = document("spec/2-kernel/218-discovery-and-findings.md");
    assert_eq!(column(&spec, "| Code |", 0), LimitationCode::NAMES);
}

#[test]
fn kinds_of_subject_are_the_ones_in_the_discovery_section() {
    let spec = document("spec/2-kernel/218-discovery-and-findings.md");
    assert_eq!(column(&spec, "| Tag |", 0), Subject::TAGS);
}

#[test]
fn availability_states_are_the_ones_in_the_provider_contract() {
    let spec = document("spec/2-kernel/203-the-provider-contract.md");
    assert_eq!(column(&spec, "| State | Meaning |", 0), Availability::NAMES);
}

/// The cell of the supervisor's rule that names a connection's health states
/// and what to do about each.
fn health_rule() -> String {
    let spec = document("spec/2-kernel/204-supervising-subsystems.md");
    let section = between(&spec, "## Long-lived connections", "## Stopping something");
    table(section, "| Rule | Detail |")
        .into_iter()
        .find(|row| row[0] == "A health state, with what to do")
        .unwrap_or_else(|| panic!("no health state row"))
        .swap_remove(1)
}

#[test]
fn subsystem_states_are_the_ones_the_supervisor_gives() {
    assert_eq!(code_words(&health_rule()), SubsystemState::NAMES);
}

#[test]
fn subsystem_moves_are_the_ones_the_supervisor_gives() {
    let rule = health_rule();
    let (_, listed) = rule.split_once("one of ").expect("the moves");
    let moves: Vec<&str> = listed
        .split(',')
        .map(|word| word.trim().trim_start_matches("or ").trim())
        .collect();
    assert_eq!(moves, SubsystemMove::NAMES);
}

#[test]
fn doctor_states_are_the_ones_doctor_reports() {
    let spec = document("spec/2-kernel/204-supervising-subsystems.md");
    let section = between(&spec, "## What `doctor` reports", "## Open points");
    // The first word written as code is the heading's own `doctor`.
    assert_eq!(code_words(section)[1..], *DoctorState::NAMES);
}

#[test]
fn identity_strategies_are_the_ones_in_the_record_identity_section() {
    let spec = document("spec/2-kernel/209-record-identity-and-dependent-records.md");
    assert_eq!(column(&spec, "| Strategy |", 0), IdentityStrategy::NAMES);
}

/// The words written as code in the meaning of one field of a file format:
/// the row `field` of the first table between two headings.
fn format_words(start: &str, end: &str, field: &str) -> Vec<String> {
    let formats = document("reference/file-formats.md");
    let section = between(&formats, start, end);
    let row = table(section, "| Field | Meaning |")
        .into_iter()
        .find(|row| row[0] == field)
        .unwrap_or_else(|| panic!("no row {field:?} after {start:?}"));
    code_words(&row[1])
}

#[test]
fn field_type_bases_are_the_ones_in_the_file_formats() {
    let listed = format_words(
        "## `types.yaml`",
        "## `screens/*.yaml`",
        "`base` (required)",
    );
    assert_eq!(listed, FieldBase::NAMES);
}

#[test]
fn sensitivity_labels_are_the_ones_in_the_file_formats() {
    let listed = format_words("## `types.yaml`", "## `screens/*.yaml`", "`sensitivity`");
    assert_eq!(listed, Sensitivity::NAMES);
}

#[test]
fn environment_kinds_are_the_ones_in_the_file_formats() {
    let listed = format_words("**Environment.**", "## `auth.yaml`", "`kind` (required)");
    assert_eq!(listed, EnvironmentKind::NAMES);
}

#[test]
fn test_kinds_are_the_ones_in_the_file_formats() {
    let listed = format_words(
        "## `tests/*.yaml` and `fixtures/`",
        "## Expressions",
        "`kind` (required)",
    );
    assert_eq!(listed, TestKind::NAMES);
}

#[test]
fn checkpoints_are_the_ones_in_the_file_formats() {
    let listed = format_words("**Step.**", "## `sources/*.yaml`", "`checkpoint`");
    assert_eq!(listed, Checkpoint::NAMES);
}

#[test]
fn sign_in_modes_are_the_ones_in_the_file_formats() {
    let listed = format_words("## `auth.yaml`", "## `types.yaml`", "`mode` (required)");
    assert_eq!(listed, a2m_core::SignInMode::NAMES);
}

#[test]
fn drift_risks_are_the_ones_in_the_file_formats() {
    let listed = format_words("## `target.yaml`", "**Surface.**", "`drift_risk`");
    assert_eq!(listed, a2m_core::DriftRisk::NAMES);
}

#[test]
fn whitespace_rules_and_decimal_marks_are_the_ones_in_the_file_formats() {
    let listed = format_words(
        "## `types.yaml`",
        "## `screens/*.yaml`",
        "`case`, `whitespace`",
    );
    assert_eq!(listed[listed.len() - 3..], *a2m_core::WhitespaceRule::NAMES);
    let listed = format_words("## `types.yaml`", "## `screens/*.yaml`", "`decimal_mark`");
    assert_eq!(listed[..2], *a2m_core::DecimalMark::NAMES);
}

#[test]
fn capture_levels_are_the_ones_in_the_traces_section() {
    let spec = document("spec/2-kernel/217-traces-evidence-and-repair.md");
    let section = between(&spec, "## Capture levels", "**When capture fails.**");
    assert_eq!(
        column(section, "| Level |", 0),
        a2m_core::CaptureLevel::NAMES
    );
}

#[test]
fn approval_rules_are_the_ones_the_configuration_section_lists() {
    let spec = document("spec/2-kernel/226-configuration.md");
    // The row of the settings table, whose second cell lists the values.
    let row: Vec<String> = spec
        .lines()
        .find(|line| line.starts_with("| `policy.approve_writes` | `"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .expect("the row of `policy.approve_writes`");
    let mut listed = code_words(&row[1]);
    listed.sort();
    let mut in_code: Vec<&str> = a2m_core::ApproveWrites::NAMES.to_vec();
    in_code.sort_unstable();
    assert_eq!(listed, in_code);
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
    round_trip(a2m_core::SignInMode::ALL, a2m_core::SignInMode::NAMES);
    round_trip(a2m_core::DriftRisk::ALL, a2m_core::DriftRisk::NAMES);
    round_trip(a2m_core::CaptureLevel::ALL, a2m_core::CaptureLevel::NAMES);
    round_trip(a2m_core::ApproveWrites::ALL, a2m_core::ApproveWrites::NAMES);
    round_trip(
        a2m_core::WhitespaceRule::ALL,
        a2m_core::WhitespaceRule::NAMES,
    );
    round_trip(a2m_core::DecimalMark::ALL, a2m_core::DecimalMark::NAMES);
    round_trip(Archetype::ALL, Archetype::NAMES);
    round_trip(Availability::ALL, Availability::NAMES);
    round_trip(Checkpoint::ALL, Checkpoint::NAMES);
    round_trip(DoctorState::ALL, DoctorState::NAMES);
    round_trip(EnvironmentKind::ALL, EnvironmentKind::NAMES);
    round_trip(FieldBase::ALL, FieldBase::NAMES);
    round_trip(IdentityStrategy::ALL, IdentityStrategy::NAMES);
    round_trip(Sensitivity::ALL, Sensitivity::NAMES);
    round_trip(SubsystemMove::ALL, SubsystemMove::NAMES);
    round_trip(SubsystemState::ALL, SubsystemState::NAMES);
    round_trip(TestKind::ALL, TestKind::NAMES);
    round_trip(a2m_core::DigestPurpose::ALL, a2m_core::DigestPurpose::NAMES);
    round_trip(a2m_core::Rounding::ALL, a2m_core::Rounding::NAMES);
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
