//! The example target compiles with the device pack, the same way every
//! time, and what the pack must refuse is refused with a file, a line, and
//! a fix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{EffectClass, Hazard};
use a2m_files::{CompileMode, Diagnostics, RelPath};
use common::{Folder, compile_files, compiled, edited, files, registry};

const MAP: &str = "registers/controller.yaml";
const DOSING: &str = "operations/dosing_rate_set.yaml";

fn problems(files: &[(RelPath, Vec<u8>)]) -> Diagnostics {
    let folder = Folder::new("compile");
    let registry = registry(&folder);
    match compile_files(files, registry.compiler_set(), CompileMode::Draft) {
        Ok(compiled) => compiled.warnings,
        Err(problems) => problems,
    }
}

/// The diagnostic with this code, which must name a file, a line, and a fix.
fn refused(files: &[(RelPath, Vec<u8>)], code: &str, file: &str) {
    let problems = problems(files);
    let found = problems
        .iter()
        .find(|found| found.code.as_str() == code && found.at.file.as_str() == file)
        .unwrap_or_else(|| panic!("no `{code}` in {file}:\n{problems}"));
    assert_eq!(found.level, a2m_files::Level::Error, "{found}");
    assert!(found.at.line >= 1 && found.at.column >= 1, "{found}");
    assert!(!found.fix.is_empty(), "{found}");
}

#[test]
fn the_example_target_compiles_twice_to_the_same_bytes() {
    let folder = Folder::new("compile");
    let registry = registry(&folder);
    let first = compiled(registry.compiler_set());
    let second = compiled(registry.compiler_set());
    assert_eq!(first.encoded.bytes, second.encoded.bytes);
    assert_eq!(first.encoded.hash, second.encoded.hash);

    let compiled_target = first.compiled_target;
    assert_eq!(compiled_target.surfaces[0].kind.as_str(), "device_link");
    // The surface carries the serial of the unit of each environment.
    let fields: serde_json::Value = compiled_target.surfaces[0].payload.decode().unwrap();
    assert_eq!(fields["serials"]["test"], "100123");
    assert_eq!(compiled_target.packs.len(), 1);
    assert_eq!(
        (
            compiled_target.packs[0].pack.as_str(),
            compiled_target.packs[0].kind.as_str()
        ),
        ("device", "register")
    );

    let operation = |name: &str| {
        compiled_target
            .operations
            .iter()
            .find(|found| found.name.as_str() == name)
            .unwrap()
    };
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };

    // A read: identify, then ask, read the reply, expect its form.
    let read = &operation("example_pump_speed_get").bindings[0];
    assert_eq!(
        kinds(&read.act),
        [
            "open",
            "write_frame",
            "read_frame",
            "expect",
            "write_frame",
            "read_frame",
            "expect"
        ]
    );
    assert!(read.act.iter().all(|step| !step.commits));

    // An update: identify, read the value as it is, build the frame and
    // give it out to be recorded, send it; the acknowledgement confirms;
    // the register is read again.
    let update = operation("example_dosing_rate_set");
    assert_eq!(
        (update.effect, update.commit_points),
        (EffectClass::Irreversible, 1)
    );
    let binding = &update.bindings[0];
    assert_eq!(
        kinds(&binding.act),
        [
            "open",
            "write_frame",
            "read_frame",
            "expect",
            "build_frame",
            "write_frame"
        ]
    );
    assert_eq!(kinds(&binding.confirm), ["read_frame", "expect"]);
    assert_eq!(
        kinds(&binding.read_back),
        ["open", "write_frame", "read_frame", "expect"]
    );
    let commits: Vec<bool> = binding.act.iter().map(|step| step.commits).collect();
    assert_eq!(commits, [false, false, false, false, false, true]);

    // A command: nothing to read before, and nothing to read back.
    let submit = operation("example_fault_reset");
    assert_eq!(
        (submit.effect, submit.hazard),
        (EffectClass::Irreversible, Hazard::DeviceFunction)
    );
    assert_eq!(
        kinds(&submit.bindings[0].act),
        ["open", "build_frame", "write_frame"]
    );
    assert!(submit.bindings[0].read_back.is_empty());
    // The device's echo is the only evidence of a command; an update keeps
    // its read-back by address.
    assert_eq!(
        submit.bindings[0].evidence,
        a2m_core::WriteEvidence::Acknowledged
    );
    assert_eq!(binding.evidence, a2m_core::WriteEvidence::ReadBack);

    // No step of this mode may be sent twice by itself.
    for operation in &compiled_target.operations {
        for binding in &operation.bindings {
            for step in binding
                .act
                .iter()
                .chain(&binding.confirm)
                .chain(&binding.read_back)
            {
                assert!(!step.idempotent, "{}", operation.name);
                assert!(step.timeout_ms == 2000, "{}", operation.name);
            }
        }
    }
}

#[test]
fn an_unknown_key_or_word_in_a_map_or_a_surface_is_refused() {
    refused(
        &edited(
            MAP,
            "    access: read\n",
            "    access: read\n    adress: 5\n",
        ),
        "kernel.unknown_key",
        MAP,
    );
    refused(
        &edited(MAP, "table: input, offset: 30", "table: memory, offset: 30"),
        "kernel.unknown_value",
        MAP,
    );
    // A serial is text, as the device reports it.
    refused(
        &edited(
            "target.yaml",
            "serials: {test: \"100123\"}",
            "serials: {test: 100123}",
        ),
        "kernel.wrong_type",
        "target.yaml",
    );
    // A serial link is not built: the surface's protocol is a closed list.
    refused(
        &edited(
            "target.yaml",
            "protocol: modbus_tcp",
            "protocol: modbus_serial",
        ),
        "kernel.unknown_value",
        "target.yaml",
    );
}

#[test]
fn a_writable_register_without_a_hazard_or_a_range_does_not_compile() {
    refused(
        &edited(
            MAP,
            "    range: {min: \"0\", max: \"100\"}\n    hazard: none\n",
            "    range: {min: \"0\", max: \"100\"}\n",
        ),
        "device.writable_without_hazard",
        MAP,
    );
    refused(
        &edited(MAP, "    range: {min: \"0\", max: \"100\"}\n", ""),
        "device.writable_without_range",
        MAP,
    );
    refused(
        &edited(
            MAP,
            "    access: write\n    hazard: device_function",
            "    access: write\n    hazard: none",
        ),
        "device.command_hazard_none",
        MAP,
    );
    // A range the register cannot hold, or that is not a whole number of
    // its steps, is refused where it is written.
    refused(
        &edited(
            MAP,
            "range: {min: \"0\", max: \"100\"}",
            "range: {min: \"0\", max: \"700\"}",
        ),
        "device.range_invalid",
        MAP,
    );
    refused(
        &edited(
            MAP,
            "range: {min: \"0\", max: \"100\"}",
            "range: {min: \"0.005\", max: \"100\"}",
        ),
        "device.range_invalid",
        MAP,
    );
    refused(
        &edited(
            MAP,
            "table: discrete, offset: 0}\n    data: bit",
            "table: discrete, offset: 0}\n    data: u16",
        ),
        "device.data_not_of_table",
        MAP,
    );
    refused(
        &edited(MAP, "pace: {min_gap: 1ms", "pace: {min_gap: soon"),
        "device.time_malformed",
        MAP,
    );
}

#[test]
fn a_binding_names_registers_of_its_map_and_declares_what_it_could_harm() {
    refused(
        &edited(
            DOSING,
            "registers: {dosing_rate: input.rate}",
            "registers: {dosing_speed: input.rate}",
        ),
        "device.unknown_register",
        DOSING,
    );
    refused(
        &edited(
            DOSING,
            "device_map: controller.main",
            "device_map: controller.other",
        ),
        "device.unknown_map",
        DOSING,
    );
    // A write through a surface that names no unit: the driver would
    // refuse it at run time, and the compiler says so first.
    refused(
        &edited("target.yaml", "    serials: {test: \"100123\"}\n", ""),
        "device.write_without_serial",
        DOSING,
    );
    // A write to a read-only register.
    refused(
        &edited(
            DOSING,
            "registers: {dosing_rate: input.rate}",
            "registers: {pump_speed_measured: input.rate}",
        ),
        "device.register_not_writable",
        DOSING,
    );
    // A write that is read back from somewhere else.
    refused(
        &edited(
            DOSING,
            "read_back: {rate: dosing_rate}",
            "read_back: {rate: pump_speed_measured}",
        ),
        "device.read_back_elsewhere",
        DOSING,
    );
    // A register whose reading clears it is never read, and never read back.
    refused(
        &edited(
            "operations/absent_point_get.yaml",
            "read: {value: absent_point}",
            "read: {value: fault_latch}",
        ),
        "device.read_has_effect",
        "operations/absent_point_get.yaml",
    );
    // A register with a hazard, written by an operation that declares none.
    refused(
        &edited(
            MAP,
            "    range: {min: \"0\", max: \"100\"}\n    hazard: none\n",
            "    range: {min: \"0\", max: \"100\"}\n    hazard: device_function\n",
        ),
        "device.hazard_not_declared",
        DOSING,
    );
    // A write that moves something is not built, and says so.
    refused(
        &edited(
            DOSING,
            "registers: {dosing_rate: input.rate}",
            "registers: {pump_speed_setpoint: input.rate}",
        ),
        "device.physical_not_built",
        DOSING,
    );
    // The fixtures themselves have one kind of warning: a draft has no
    // offline tests yet.
    let clean = problems(&files());
    assert!(!clean.has_errors(), "{clean}");
    assert!(
        clean
            .iter()
            .all(|found| found.code.as_str() == "kernel.operation_without_test"),
        "{clean}"
    );
}

#[test]
fn a_write_never_compiles_as_a_read_and_a_command_never_as_a_plain_write() {
    // What the pack says each verb does, whatever the operation's file
    // says: an update writes.
    let found = problems(&edited(DOSING, "effect: irreversible", "effect: read"));
    assert!(
        found.iter().any(
            |found| found.code.as_str() == "kernel.effect_lower_than_derived"
                && found.at.file.as_str() == DOSING
        ),
        "{found}"
    );
    // A command cannot be read back, so `write` is too little for it.
    let command = "operations/fault_reset.yaml";
    let found = problems(&edited(command, "effect: irreversible", "effect: write"));
    let lower = found
        .iter()
        .find(|found| {
            found.code.as_str() == "kernel.effect_lower_than_derived"
                && found.at.file.as_str() == command
        })
        .unwrap_or_else(|| panic!("{found}"));
    assert!(lower.fix.contains("irreversible"), "{lower}");
}

#[test]
fn every_tool_states_that_it_starts_nothing_and_sends_only_over_its_link() {
    use a2m_files::{BindingStates, Reach};
    let folder = Folder::new("states");
    let registry = registry(&folder);
    let compiled = compiled(registry.compiler_set());
    assert!(!compiled.review.items.is_empty());
    for item in &compiled.review.items {
        let Reach::Stated { groups, .. } = &item.reach else {
            panic!("`{}` does not state where it sends", item.tool);
        };
        let addresses: Vec<&str> = groups
            .iter()
            .flat_map(|group| group.addresses.iter().map(String::as_str))
            .collect();
        assert_eq!(
            addresses,
            ["127.0.0.1:1502", "127.0.0.1:502"],
            "{}",
            item.tool
        );
        assert!(!item.bindings.is_empty());
        for binding in &item.bindings {
            let BindingStates::Stated { runs, environment } = &binding.states else {
                panic!("`{}` is not stated", item.tool);
            };
            assert!(runs.is_empty() && environment.is_empty(), "{}", item.tool);
        }
    }
}
