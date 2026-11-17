//! The snapshot of a page, built from the browser's accessibility tree in
//! the form of the reference document "Steps, snapshots, and diffs". A
//! run's evidence and discovery's findings use this one builder. Which
//! screen models match is added by the crate that knows screen models.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use a2m_core::Redact;

use crate::protocol::AxNode;

/// How much a snapshot may hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotLimits {
    /// The most elements. A larger page is cut, deepest content first.
    pub max_elements: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self { max_elements: 400 }
    }
}

/// A state of an element that a person would perceive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementState {
    /// It cannot be used.
    Disabled,
    /// It is checked, or partly checked.
    Checked,
    /// It is expanded.
    Expanded,
    /// It must be filled.
    Required,
    /// The page marks its value as invalid.
    Invalid,
}

impl ElementState {
    fn word(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Checked => "checked",
            Self::Expanded => "expanded",
            Self::Required => "required",
            Self::Invalid => "invalid",
        }
    }
}

/// One element of a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotElement {
    /// A reference, stable within this snapshot only, such as `e3`.
    pub reference: String,
    /// The accessibility role; `text` for text that belongs to no named
    /// element.
    pub role: String,
    /// The accessible name.
    pub name: String,
    /// The value, where the element has one. The value of a password
    /// input is [`MASK`].
    pub value: Option<String>,
    /// Its states, in a fixed order.
    pub states: Vec<ElementState>,
    /// The level of a heading.
    pub level: Option<u32>,
    /// How deep it is among the elements kept.
    pub depth: usize,
}

/// A compact view of a page: what a person using assistive technology
/// would be told. Hidden elements and decoration are left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The page's address.
    pub address: String,
    /// The page's title.
    pub title: String,
    /// The elements, in document order.
    pub elements: Vec<SnapshotElement>,
    /// Whether deeper content was cut to fit the limit.
    pub cut: bool,
}

/// What stands in a snapshot for the value of a password input.
pub const MASK: &str = "••••";

/// Roles that carry no meaning of their own; their children are kept.
const TRANSPARENT: &[&str] = &["", "none", "generic", "RootWebArea", "presentation"];

/// Roles left out with everything under them: the pieces text is laid out
/// in, and a label, whose text is the name of its control.
const LEFT_OUT: &[&str] = &["InlineTextBox", "LineBreak", "LabelText", "ListMarker"];

fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Snapshot {
    pub(crate) fn build(
        address: String,
        title: String,
        nodes: &[AxNode],
        masked: &BTreeSet<i64>,
        limits: &SnapshotLimits,
        redact: &dyn Redact,
    ) -> Self {
        let by_id: BTreeMap<&str, &AxNode> =
            nodes.iter().map(|node| (node.node_id.as_str(), node)).collect();
        let mut elements = Vec::new();
        // Depth first, in the order the tree gives its children. Each entry
        // is a node, its depth among kept elements, and whether an element
        // above it already carries its text as a name.
        let mut stack: Vec<(&AxNode, usize, bool)> = nodes
            .iter()
            .filter(|node| node.parent_id.is_none())
            .rev()
            .map(|node| (node, 0, false))
            .collect();
        // A tree that names a node as its own descendant must not loop.
        let mut budget = nodes.len();
        while let Some((node, depth, named_above)) = stack.pop() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let role = node.role.as_ref().map(|role| role.text()).unwrap_or_default();
            if LEFT_OUT.contains(&role.as_str()) {
                continue;
            }
            let name = node.name.as_ref().map(|name| name.text()).unwrap_or_default();
            let transparent = node.ignored || TRANSPARENT.contains(&role.as_str());
            let text = role == "StaticText";
            let kept = !transparent && !(text && named_above) && !(text && name.trim().is_empty());
            if kept {
                let property = |wanted: &str| {
                    node.properties
                        .iter()
                        .find(|property| property.name == wanted)
                        .map(|property| property.value.text())
                };
                let mut states = Vec::new();
                if property("disabled").as_deref() == Some("true") {
                    states.push(ElementState::Disabled);
                }
                if matches!(property("checked").as_deref(), Some("true" | "mixed")) {
                    states.push(ElementState::Checked);
                }
                if property("expanded").as_deref() == Some("true") {
                    states.push(ElementState::Expanded);
                }
                if property("required").as_deref() == Some("true") {
                    states.push(ElementState::Required);
                }
                if property("invalid").is_some_and(|invalid| invalid != "false") {
                    states.push(ElementState::Invalid);
                }
                elements.push(SnapshotElement {
                    reference: String::new(),
                    role: if text { "text".to_owned() } else { role },
                    name: redact.redact(&name),
                    value: node
                        .value
                        .as_ref()
                        .map(|value| value.text())
                        .filter(|value| !value.is_empty())
                        .map(|value| {
                            let password = node
                                .backend_dom_node_id
                                .is_some_and(|number| masked.contains(&number));
                            if password {
                                MASK.to_owned()
                            } else {
                                redact.redact(&value)
                            }
                        }),
                    states,
                    level: property("level").and_then(|level| level.parse().ok()),
                    depth,
                });
            }
            let below = if kept { depth + 1 } else { depth };
            let named = named_above || (kept && !name.trim().is_empty());
            for child in node.child_ids.iter().rev() {
                if let Some(child) = by_id.get(child.as_str()) {
                    stack.push((child, below, named));
                }
            }
        }

        // Cut deepest content first: keep the levels that fit whole.
        let mut cut = false;
        if elements.len() > limits.max_elements {
            cut = true;
            let deepest = elements.iter().map(|element| element.depth).max().unwrap_or(0);
            let fits = (0..=deepest).rev().find(|level| {
                elements.iter().filter(|element| element.depth <= *level).count()
                    <= limits.max_elements
            });
            match fits {
                Some(level) => elements.retain(|element| element.depth <= level),
                None => {
                    elements.retain(|element| element.depth == 0);
                    elements.truncate(limits.max_elements);
                }
            }
        }
        for (index, element) in elements.iter_mut().enumerate() {
            element.reference = format!("e{}", index + 1);
        }
        Self {
            address,
            title,
            elements,
            cut,
        }
    }

    /// The text form shown to agents: one element per line, indented by
    /// depth, reference first.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = format!("page {} {}\n", quoted(&self.title), self.address);
        for element in &self.elements {
            let _ = write!(
                out,
                "{:indent$}{} {} {}",
                "",
                element.reference,
                element.role,
                quoted(&element.name),
                indent = 2 * (element.depth + 1)
            );
            if let Some(value) = &element.value {
                let _ = write!(out, " value={}", quoted(value));
            }
            if let Some(level) = element.level {
                let _ = write!(out, " level={level}");
            }
            for state in &element.states {
                let _ = write!(out, " {}", state.word());
            }
            out.push('\n');
        }
        if self.cut {
            out.push_str("cut: deeper content was left out to fit the limit\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::NoSecrets;

    use super::*;
    use crate::protocol::AxNodes;

    fn tree() -> Vec<AxNode> {
        let nodes: AxNodes = serde_json::from_str(
            r#"{"nodes":[
            {"nodeId":"1","role":{"value":"RootWebArea"},"name":{"value":"Order A-100"},"childIds":["2","3","9"]},
            {"nodeId":"2","parentId":"1","role":{"value":"heading"},"name":{"value":"Line \"Item\" Details"},
             "properties":[{"name":"level","value":{"value":2}}],"childIds":["4"]},
            {"nodeId":"4","parentId":"2","role":{"value":"StaticText"},"name":{"value":"Line Item Details"},"childIds":["5"]},
            {"nodeId":"5","parentId":"4","role":{"value":"InlineTextBox"},"name":{"value":"Line Item Details"}},
            {"nodeId":"3","parentId":"1","ignored":true,"role":{"value":"none"},"childIds":["6","7","8"]},
            {"nodeId":"6","parentId":"3","role":{"value":"LabelText"},"childIds":["10"]},
            {"nodeId":"10","parentId":"6","role":{"value":"StaticText"},"name":{"value":"Item code"}},
            {"nodeId":"7","parentId":"3","backendDOMNodeId":70,"role":{"value":"textbox"},"name":{"value":"Item code"},"value":{"value":"L-101"},
             "properties":[{"name":"required","value":{"value":true}},{"name":"invalid","value":{"value":"false"}}]},
            {"nodeId":"8","parentId":"3","role":{"value":"paragraph"},"name":{"value":""},"childIds":["11"]},
            {"nodeId":"11","parentId":"8","role":{"value":"StaticText"},"name":{"value":"inches"}},
            {"nodeId":"9","parentId":"1","role":{"value":"button"},"name":{"value":"Save"},
             "properties":[{"name":"disabled","value":{"value":true}}]}
            ]}"#,
        )
        .unwrap();
        nodes.nodes
    }

    #[test]
    fn a_snapshot_keeps_what_a_person_is_told_and_leaves_out_decoration() {
        let snapshot = Snapshot::build(
            "https://app.example.com/orders/A-100".to_owned(),
            "Order A-100".to_owned(),
            &tree(),
            &BTreeSet::new(),
            &SnapshotLimits::default(),
            &NoSecrets,
        );
        assert!(!snapshot.cut);
        assert_eq!(
            snapshot.to_text(),
            "page \"Order A-100\" https://app.example.com/orders/A-100\n\
             \x20 e1 heading \"Line \\\"Item\\\" Details\" level=2\n\
             \x20 e2 textbox \"Item code\" value=\"L-101\" required\n\
             \x20 e3 paragraph \"\"\n\
             \x20   e4 text \"inches\"\n\
             \x20 e5 button \"Save\" disabled\n"
        );
    }

    #[test]
    fn a_snapshot_over_its_limit_is_cut_deepest_first_and_says_so() {
        let snapshot = Snapshot::build(
            String::new(),
            String::new(),
            &tree(),
            &BTreeSet::from([70]),
            &SnapshotLimits { max_elements: 4 },
            &NoSecrets,
        );
        // The node the engine was told is a password input shows no value.
        assert_eq!(snapshot.elements[1].value.as_deref(), Some(MASK));
        assert!(snapshot.cut);
        let roles: Vec<&str> = snapshot.elements.iter().map(|e| e.role.as_str()).collect();
        assert_eq!(roles, ["heading", "textbox", "paragraph", "button"]);
        assert!(snapshot.to_text().ends_with("to fit the limit\n"));
    }
}
