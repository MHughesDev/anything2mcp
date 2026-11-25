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
    /// For a table, how many columns its system states it has, when it
    /// states it (on Linux, AT-SPI's Table interface): what tells where a
    /// row ends in a table whose cells are its own children (decision
    /// 0999).
    pub columns: Option<u32>,
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

    /// The headings and the rows of cells of the table at `at`, or why
    /// they cannot be read. A table holds rows, one of them of headings
    /// only; or, as GTK 3 publishes one, its column headers and then its
    /// cells as its own children, with no row between, which is read only
    /// when the table states how many columns it has: then the first that
    /// many are the headings and each run of that many after them is one
    /// row. Nothing tells where a row ends in a table that does not state
    /// it, and nothing is guessed (rule 7; decision 0999).
    pub(crate) fn table(&self, at: usize) -> Result<(Vec<String>, Vec<Vec<usize>>), &'static str> {
        let table = &self.nodes[at];
        let children = &table.children;
        let rows = children
            .iter()
            .filter(|child| self.nodes[**child].role == "table_row")
            .count();
        let mut header = None;
        let mut body = Vec::new();
        if rows == children.len() {
            for row in children {
                let cells: Vec<usize> = self.nodes[*row].children.clone();
                if cells
                    .iter()
                    .all(|cell| self.nodes[*cell].role == "table_column_header")
                {
                    if header.is_some() {
                        return Err("has more than one row of headings");
                    }
                    header = Some(
                        cells
                            .iter()
                            .map(|cell| self.nodes[*cell].name.clone())
                            .collect::<Vec<_>>(),
                    );
                } else {
                    body.push(cells);
                }
            }
        } else if rows > 0 {
            return Err("holds rows and, beside them, something that is not a row");
        } else {
            let Some(count) = table.columns.and_then(|count| usize::try_from(count).ok()) else {
                return Err(
                    "holds its cells with no row between them, and does not state how many columns it has",
                );
            };
            let (heads, cells) = children.split_at(count.min(children.len()));
            if heads.len() != count
                || heads
                    .iter()
                    .any(|head| self.nodes[*head].role != "table_column_header")
            {
                return Err("does not begin with one heading for each column it states");
            }
            if !cells.len().is_multiple_of(count) {
                return Err("holds a number of cells that is not a whole number of rows");
            }
            header = Some(
                heads
                    .iter()
                    .map(|head| self.nodes[*head].name.clone())
                    .collect(),
            );
            body = cells.chunks(count).map(<[usize]>::to_vec).collect();
        }
        let header = header.ok_or("shows no row of headings")?;
        for cells in &body {
            if cells.len() != header.len()
                || cells
                    .iter()
                    .any(|cell| self.nodes[*cell].role != "table_cell")
            {
                return Err("has a row without one cell under each heading");
            }
        }
        Ok((header, body))
    }

    /// The one table whose headings are exactly `header`: how a table the
    /// application gives no name is found. Two such tables stop the search.
    pub(crate) fn table_headed(&self, header: &[String]) -> Result<usize, Miss> {
        let found: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(at, node)| {
                node.role == "table"
                    && self
                        .table(*at)
                        .is_ok_and(|(headings, _)| headings.as_slice() == header)
            })
            .map(|(at, _)| at)
            .collect();
        match found.as_slice() {
            [] => Err(Miss::None),
            [only] => Ok(*only),
            many => Err(Miss::Many(many.len())),
        }
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
            columns: None,
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

    fn tree_of(nodes: Vec<(u32, &'static str, &str)>, columns: Option<u32>) -> Tree {
        let mut tree = Tree::default();
        let mut parents: Vec<usize> = Vec::new();
        for (depth, role, name) in nodes {
            let mut one = node(role, name);
            one.depth = depth;
            if role == "table" {
                one.columns = columns;
            }
            let at = tree.nodes.len();
            parents.truncate(usize::try_from(depth).unwrap());
            if let Some(parent) = parents.last() {
                tree.nodes[*parent].children.push(at);
            }
            parents.push(at);
            tree.nodes.push(one);
        }
        tree
    }

    #[test]
    fn a_table_whose_cells_are_its_own_children_is_read_by_the_column_count_it_states() {
        let cells = vec![
            (0, "panel", ""),
            (1, "table", ""),
            (2, "table_column_header", "Line"),
            (2, "table_column_header", "Item"),
            (2, "table_cell", "1"),
            (2, "table_cell", "A"),
            (2, "table_cell", "2"),
            (2, "table_cell", "B"),
        ];
        let tree = tree_of(cells.clone(), Some(2));
        let (header, body) = tree.table(1).unwrap();
        assert_eq!(header, ["Line", "Item"]);
        assert_eq!(body, [vec![4, 5], vec![6, 7]]);
        let headed = ["Line".to_owned(), "Item".to_owned()];
        assert_eq!(tree.table_headed(&headed), Ok(1));
        assert_eq!(tree.table_headed(&headed[..1]), Err(Miss::None));
        // Stated by nothing, no row is guessed.
        let silent = tree_of(cells.clone(), None);
        assert!(silent.table(1).unwrap_err().contains("does not state"));
        assert_eq!(silent.table_headed(&headed), Err(Miss::None));
        // A count the cells do not fill, or headings it does not begin
        // with, is a stop.
        assert!(tree_of(cells.clone(), Some(3)).table(1).is_err());
        let mut short = cells.clone();
        short.pop();
        assert!(tree_of(short, Some(2)).table(1).is_err());
        // Rows, and something beside them, is a stop too.
        let mixed = vec![
            (0, "table", "Lines"),
            (1, "table_row", ""),
            (2, "table_column_header", "Line"),
            (1, "table_cell", "1"),
        ];
        assert!(tree_of(mixed, Some(1)).table(0).is_err());
        // Rows read as before, the count or not.
        let rows = vec![
            (0, "table", "Lines"),
            (1, "table_row", ""),
            (2, "table_column_header", "Line"),
            (1, "table_row", ""),
            (2, "table_cell", "1"),
        ];
        assert_eq!(
            tree_of(rows, None).table(0).unwrap(),
            (vec!["Line".to_owned()], vec![vec![4]])
        );
        // Two tables of the same headings cannot be told apart.
        let mut two = cells.clone();
        two.extend(cells[1..].iter().cloned());
        assert_eq!(
            tree_of(two, Some(2)).table_headed(&headed),
            Err(Miss::Many(2))
        );
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
