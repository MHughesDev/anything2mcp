//! The roles a desktop screen may locate by: the pack's own words, each of
//! which a back end maps from its system's roles. A role the list does not
//! hold cannot be written in a screen, and an element whose system role
//! maps to none of them is shown as `other` and never matched.

/// Every role word, in the order of the table in the pack's design.
pub(crate) const ROLES: &[&str] = &[
    "application",
    "frame",
    "window",
    "dialog",
    "panel",
    "filler",
    "scroll_pane",
    "viewport",
    "section",
    "form",
    "tool_bar",
    "status_bar",
    "menu_bar",
    "menu",
    "menu_item",
    "page_tab_list",
    "page_tab",
    "heading",
    "paragraph",
    "label",
    "static",
    "image",
    "entry",
    "text",
    "password_text",
    "spin_button",
    "combo_box",
    "check_box",
    "radio_button",
    "push_button",
    "toggle_button",
    "list",
    "list_box",
    "list_item",
    "tree",
    "tree_item",
    "table",
    "table_row",
    "table_cell",
    "table_column_header",
    "table_row_header",
];

/// The word for an element whose role is none of [`ROLES`].
#[cfg(any(target_os = "linux", test))]
pub(crate) const OTHER: &str = "other";

/// The roles a window of an application may have.
pub(crate) const WINDOW_ROLES: &[&str] = &["frame", "window", "dialog"];

/// The roles whose accessible name is the shape of the window, not a value
/// it shows: a window's title, a container's caption, a control's label, a
/// heading of a table's column. A snapshot keeps the name of these only;
/// the name of a cell, a row, a list item, a paragraph, or a label that
/// labels nothing is what the window shows, and is never kept.
pub(crate) const NAMED_BY_SHAPE: &[&str] = &[
    "frame",
    "window",
    "dialog",
    "panel",
    "section",
    "form",
    "tool_bar",
    "menu_bar",
    "menu",
    "menu_item",
    "page_tab_list",
    "page_tab",
    "heading",
    "entry",
    "text",
    "password_text",
    "spin_button",
    "combo_box",
    "check_box",
    "radio_button",
    "push_button",
    "toggle_button",
    "list",
    "list_box",
    "tree",
    "table",
    "table_column_header",
];

/// Whether `role` is a word a screen may locate by.
pub(crate) fn known(role: &str) -> bool {
    ROLES.contains(&role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_list_is_made_of_known_words() {
        for role in NAMED_BY_SHAPE.iter().chain(WINDOW_ROLES) {
            assert!(known(role), "{role}");
        }
        assert!(!known(OTHER));
        let mut sorted = ROLES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ROLES.len());
    }
}
