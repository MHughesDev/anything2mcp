//! One read of a window's accessibility tree, as every system's back end
//! gives it, and what the driver decides from it: which element a locator
//! means, and whether a condition holds. Nothing here reaches a system.

use crate::model::{Cond, Locator};
use crate::roles;

/// Where an element lives on its system's interface: opaque to everything
/// but the back end that made it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct El {
    pub bus: String,
    pub path: String,
}

/// One element, as one walk saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    /// How far below the walk's root: the root is 0.
    pub depth: u32,
    /// The pack's word for its role.
    pub role: &'static str,
    /// Its accessible name: its own, else that of what labels it.
    pub name: String,
    /// Whether the system says it is shown.
    pub showing: bool,
    /// Whether its role or state marks it as holding a secret. Its value
    /// is never read.
    pub secret: bool,
    /// Whether it gives its value as text, apart from its name.
    pub has_text: bool,
    /// Whether it labels another element: then its name is the window's
    /// shape, not a value.
    pub labels: bool,
    /// The elements directly under it, by position in the walk.
    pub children: Vec<usize>,
    /// Where it lives.
    pub el: El,
}

/// What one walk saw, in the order it saw it: each element before the
/// elements under it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Tree {
    pub nodes: Vec<Node>,
    /// The bound the walk stopped at, when it stopped early: `nodes`,
    /// `depth`, or `seconds`.
    pub cut: Option<&'static str>,
}

/// How many elements a locator matched, when it did not match exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Miss {
    None,
    Many(usize),
}

impl Tree {
    /// Every element of the role and the name, exactly.
    pub(crate) fn matching(&self, locator: &Locator) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.role == locator.role && node.name == locator.name)
            .map(|(at, _)| at)
            .collect()
    }

    /// The one element the locators mean: the first locator, in order,
    /// that matches anything, and it must match exactly one. A locator
    /// that matches several stops the search there; nothing is guessed.
    pub(crate) fn one(&self, locators: &[Locator]) -> Result<usize, Miss> {
        for locator in locators {
            match self.matching(locator).as_slice() {
                [] => {}
                [only] => return Ok(*only),
                many => return Err(Miss::Many(many.len())),
            }
        }
        Err(Miss::None)
    }

    /// Whether a condition holds on this read, or the element a `text`
    /// condition names cannot be told apart. A `text` condition's value is
    /// read by `text_of`.
    pub(crate) fn holds<F>(&self, cond: &Cond, text_of: &mut F) -> Result<bool, Miss>
    where
        F: FnMut(&Node) -> Option<String>,
    {
        match cond {
            Cond::Visible(locator) => Ok(self
                .matching(locator)
                .iter()
                .any(|at| self.nodes[*at].showing)),
            Cond::Text {
                locate,
                text,
                contains,
            } => match self.one(std::slice::from_ref(locate)) {
                Ok(at) => Ok(text_of(&self.nodes[at]).is_some_and(|shown| {
                    if *contains {
                        shown.contains(text.as_str())
                    } else {
                        shown == *text
                    }
                })),
                Err(Miss::None) => Ok(false),
                Err(many) => Err(many),
            },
            Cond::All(all) => {
                for one in all {
                    if !self.holds(one, text_of)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    /// The name a snapshot may keep for an element: the window's shape,
    /// never a value it shows.
    pub(crate) fn shape_name(node: &Node) -> Option<&str> {
        let by_shape =
            roles::NAMED_BY_SHAPE.contains(&node.role) || (node.role == "label" && node.labels);
        (by_shape && !node.name.is_empty()).then_some(node.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(role: &'static str, name: &str) -> Node {
        Node {
            depth: 1,
            role,
            name: name.to_owned(),
            showing: true,
            secret: false,
            has_text: false,
            labels: false,
            children: Vec::new(),
            el: El {
                bus: ":1.1".to_owned(),
                path: "/x".to_owned(),
            },
        }
    }

    fn locator(role: &str, name: &str) -> Locator {
        Locator {
            role: role.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn a_locator_means_one_element_or_stops() {
        let tree = Tree {
            nodes: vec![
                node("entry", "Reference"),
                node("entry", "Reference"),
                node("entry", "Customer"),
                node("entry", ""),
            ],
            cut: None,
        };
        assert_eq!(tree.one(&[locator("entry", "Customer")]), Ok(2));
        assert_eq!(
            tree.one(&[locator("entry", "Reference"), locator("entry", "Customer")]),
            Err(Miss::Many(2))
        );
        assert_eq!(tree.one(&[locator("entry", "Nowhere")]), Err(Miss::None));
        assert_eq!(
            tree.one(&[locator("entry", "Nowhere"), locator("entry", "Customer")]),
            Ok(2)
        );
        // A role is matched exactly: no nearest role, no part of a name.
        assert_eq!(tree.one(&[locator("text", "Customer")]), Err(Miss::None));
        assert_eq!(tree.one(&[locator("entry", "Cust")]), Err(Miss::None));
    }

    #[test]
    fn a_snapshot_keeps_the_names_of_the_shape_only() {
        let mut label = node("label", "Not saved");
        assert_eq!(Tree::shape_name(&label), None);
        label.labels = true;
        assert_eq!(Tree::shape_name(&label), Some("Not saved"));
        assert_eq!(Tree::shape_name(&node("table_cell", "L-101")), None);
        assert_eq!(
            Tree::shape_name(&node("table_column_header", "Item code")),
            Some("Item code")
        );
    }
}
