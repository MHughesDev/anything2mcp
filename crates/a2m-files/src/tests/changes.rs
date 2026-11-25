//! Decision 0439, point 2: each change on the closed list is breaking, and
//! each other change is compatible. The two versions are the `example`
//! target compiled once, and a copy changed in one place for each case.

use a2m_core::{CodeLabel, CompiledTarget, EffectClass, Hazard, OperationName, ValueType, Version};

use super::example::example_files;
use crate::testing::stand_set;
use crate::{Change, ChangeClass, CompileMode, TargetRoot, changes_between, compile, load_files};

const GET: &str = "example_order_get";
const UPSERT: &str = "example_line_item_upsert";

fn published() -> CompiledTarget {
    let set = stand_set().unwrap();
    let sources = load_files(&TargetRoot::new("example"), &example_files(), &set).unwrap();
    compile(&sources, &set, CompileMode::Publish, Version::new(1, 0, 0))
        .unwrap()
        .compiled_target
}

fn operation<'a>(
    compiled_target: &'a mut CompiledTarget,
    name: &str,
) -> &'a mut a2m_core::CompiledOperation {
    compiled_target
        .operations
        .iter_mut()
        .find(|operation| operation.name.as_str() == name)
        .unwrap()
}

/// What changed in `tool` when `edit` is made to a copy of the version.
fn changes_of(tool: &str, edit: impl FnOnce(&mut CompiledTarget)) -> (ChangeClass, Vec<Change>) {
    let before = published();
    let mut after = before.clone();
    after.header.version = Version::new(2, 0, 0);
    edit(&mut after);
    let changes = changes_between(&before, &after);
    assert_eq!(changes.from, Version::new(1, 0, 0));
    assert_eq!(changes.to, Version::new(2, 0, 0));
    let found = changes
        .of(&OperationName::new(tool).unwrap())
        .unwrap()
        .clone();
    (found.class, found.changes)
}

fn breaks(tool: &str, edit: impl FnOnce(&mut CompiledTarget), expected: &Change) {
    let (class, changes) = changes_of(tool, edit);
    assert_eq!(class, ChangeClass::Breaking, "{changes:?}");
    assert!(changes.contains(expected), "{expected:?} in {changes:?}");
    assert!(expected.breaking());
    // The breaking changes come first.
    let first_compatible = changes.iter().position(|change| !change.breaking());
    if let Some(at) = first_compatible {
        assert!(changes[at..].iter().all(|change| !change.breaking()));
    }
}

fn compatible(tool: &str, edit: impl FnOnce(&mut CompiledTarget), expected: &Change) {
    let (class, changes) = changes_of(tool, edit);
    assert_eq!(class, ChangeClass::Compatible, "{changes:?}");
    assert_eq!(changes, vec![expected.clone()]);
    assert!(!expected.breaking());
}

#[test]
fn the_same_version_twice_changes_nothing() {
    let before = published();
    let changes = changes_between(&before, &before);
    assert!(!changes.tools.is_empty());
    assert!(
        changes
            .tools
            .iter()
            .all(|tool| tool.class == ChangeClass::Unchanged && tool.changes.is_empty())
    );
    assert_eq!(changes.breaking().count(), 0);
}

#[test]
fn a_tool_gone_is_breaking_and_a_new_one_compatible_and_the_breaking_come_first() {
    let before = published();
    let mut after = before.clone();
    let at = after
        .tools
        .iter()
        .position(|tool| tool.name.as_str() == GET)
        .unwrap();
    let mut renamed = after.tools.remove(at);
    renamed.name = OperationName::new("example_order_read").unwrap();
    after.tools.push(renamed);
    let changes = changes_between(&before, &after);
    assert_eq!(changes.tools[0].tool.as_str(), GET);
    assert_eq!(changes.tools[0].class, ChangeClass::Breaking);
    assert_eq!(changes.tools[0].changes, vec![Change::ToolGone]);
    let added = changes
        .of(&OperationName::new("example_order_read").unwrap())
        .unwrap();
    assert_eq!(added.class, ChangeClass::Compatible);
    assert_eq!(added.changes, vec![Change::ToolAdded]);
    let classes: Vec<ChangeClass> = changes.tools.iter().map(|tool| tool.class).collect();
    let mut sorted = classes.clone();
    sorted.sort();
    assert_eq!(classes, sorted);
}

#[test]
fn each_change_on_the_closed_list_is_breaking() {
    breaks(
        UPSERT,
        |after| {
            operation(after, UPSERT)
                .inputs
                .retain(|input| input.name != "length");
        },
        &Change::InputGone {
            input: "length".to_owned(),
        },
    );
    breaks(
        GET,
        |after| {
            let mut extra = operation(after, GET).inputs[0].clone();
            extra.name = "channel".to_owned();
            extra.required = true;
            extra.default = None;
            operation(after, GET).inputs.push(extra);
        },
        &Change::InputRequired {
            input: "channel".to_owned(),
        },
    );
    breaks(
        GET,
        |after| operation(after, GET).inputs[0].ty = ValueType::Int,
        &Change::InputTypeChanged {
            input: "order_id".to_owned(),
            from: "text".to_owned(),
            to: "int".to_owned(),
        },
    );
    breaks(
        GET,
        |after| {
            operation(after, GET)
                .outputs
                .retain(|output| output.name != "note");
        },
        &Change::OutputGone {
            output: "note".to_owned(),
        },
    );
    breaks(
        GET,
        |after| {
            let output = operation(after, GET)
                .outputs
                .iter_mut()
                .find(|output| output.name == "total")
                .unwrap();
            output.may_be_absent = true;
        },
        &Change::OutputMayBeAbsent {
            output: "total".to_owned(),
        },
    );
    breaks(
        GET,
        |after| {
            let output = operation(after, GET)
                .outputs
                .iter_mut()
                .find(|output| output.name == "note")
                .unwrap();
            output.ty = ValueType::list_of(ValueType::Text);
        },
        &Change::OutputTypeChanged {
            output: "note".to_owned(),
            from: "text".to_owned(),
            to: "list of text".to_owned(),
        },
    );
    breaks(
        GET,
        |after| operation(after, GET).effect = EffectClass::Write,
        &Change::EffectRose {
            from: EffectClass::Read,
            to: EffectClass::Write,
        },
    );
    breaks(
        UPSERT,
        |after| operation(after, UPSERT).hazard = Hazard::DataElsewhere,
        &Change::HazardChanged {
            from: Hazard::None,
            to: Hazard::DataElsewhere,
        },
    );
    breaks(
        UPSERT,
        |after| {
            operation(after, UPSERT).identity.key.pop();
        },
        &Change::KeyChanged {
            from: vec!["order_id".to_owned(), "item_code".to_owned()],
            to: vec!["order_id".to_owned()],
        },
    );
}

/// An input whose field type is a choice: the `order_status` codes, given
/// to the order's key in both versions so only the words differ.
fn with_words(after: &mut CompiledTarget, codes: &[&str]) {
    let status = after
        .field_types
        .iter()
        .position(|field_type| field_type.name == "order_status")
        .unwrap();
    after.field_types[status].codes = codes
        .iter()
        .map(|code| CodeLabel {
            code: (*code).to_owned(),
            label: (*code).to_owned(),
        })
        .collect();
}

#[test]
fn words_narrowed_break_and_words_widened_do_not() {
    let mut before = published();
    let status = before
        .field_types
        .iter()
        .position(|field_type| field_type.name == "order_status")
        .unwrap();
    let input = &mut operation(&mut before, GET).inputs[0];
    input.field_type = a2m_core::FieldTypeIndex::new(u32::try_from(status).unwrap());
    input.ty = ValueType::Choice;
    let mut narrowed = before.clone();
    with_words(&mut narrowed, &["draft"]);
    let changes = changes_between(&before, &narrowed);
    let get = changes.of(&OperationName::new(GET).unwrap()).unwrap();
    assert_eq!(get.class, ChangeClass::Breaking);
    assert!(get.changes.contains(&Change::InputWordsNarrowed {
        input: "order_id".to_owned(),
        removed: vec!["released".to_owned()],
    }));
    let mut widened = before.clone();
    with_words(&mut widened, &["draft", "released", "closed"]);
    let changes = changes_between(&before, &widened);
    let get = changes.of(&OperationName::new(GET).unwrap()).unwrap();
    assert!(get.changes.contains(&Change::InputWordsWidened {
        input: "order_id".to_owned(),
        added: vec!["closed".to_owned()],
    }));
    // The output `status` holds the same words: wider words there are
    // compatible too.
    assert_eq!(get.class, ChangeClass::Compatible, "{:?}", get.changes);
}

#[test]
fn every_other_change_is_compatible() {
    compatible(
        GET,
        |after| {
            let mut extra = operation(after, GET).inputs[0].clone();
            extra.name = "channel".to_owned();
            extra.required = false;
            operation(after, GET).inputs.push(extra);
        },
        &Change::InputAdded {
            input: "channel".to_owned(),
        },
    );
    compatible(
        GET,
        |after| {
            let mut extra = operation(after, GET).outputs[0].clone();
            extra.name = "placed_on".to_owned();
            operation(after, GET).outputs.push(extra);
        },
        &Change::OutputAdded {
            output: "placed_on".to_owned(),
        },
    );
    compatible(
        UPSERT,
        |after| operation(after, UPSERT).effect = EffectClass::Read,
        &Change::EffectLowered {
            from: EffectClass::Write,
            to: EffectClass::Read,
        },
    );
    compatible(
        GET,
        |after| {
            let tool = after
                .tools
                .iter_mut()
                .find(|tool| tool.name.as_str() == GET)
                .unwrap();
            tool.description.push_str(" It reads nothing else.");
        },
        &Change::DescriptionChanged,
    );
    compatible(
        GET,
        |after| {
            let tool = after
                .tools
                .iter_mut()
                .find(|tool| tool.name.as_str() == GET)
                .unwrap();
            tool.title = "Read an order".to_owned();
        },
        &Change::DefinitionChanged,
    );
}

#[test]
fn an_input_made_optional_loosens_and_a_required_one_with_a_default_does_not_break() {
    compatible(
        UPSERT,
        |after| operation(after, UPSERT).inputs[2].required = false,
        &Change::InputLoosened {
            input: "length".to_owned(),
        },
    );
    compatible(
        GET,
        |after| {
            let mut extra = operation(after, GET).inputs[0].clone();
            extra.name = "channel".to_owned();
            extra.required = true;
            extra.default = Some(a2m_core::Value::Text("web".to_owned()));
            operation(after, GET).inputs.push(extra);
        },
        &Change::InputAdded {
            input: "channel".to_owned(),
        },
    );
}

#[test]
fn the_review_summary_carries_the_comparison_and_its_text_is_redacted() {
    let before = published();
    let mut after = before.clone();
    operation(&mut after, UPSERT)
        .inputs
        .retain(|input| input.name != "length");
    let mut review = crate::ReviewSummary::default();
    assert!(review.changes.is_none());
    review.compare_with(&before, &after);
    let shown = review.map_text(&|text| text.replace("length", "[secret]"));
    let changes = shown.changes.unwrap();
    let upsert = changes.breaking().next().unwrap();
    assert_eq!(upsert.tool.as_str(), UPSERT);
    assert_eq!(
        upsert.changes,
        vec![Change::InputGone {
            input: "[secret]".to_owned()
        }]
    );
}
