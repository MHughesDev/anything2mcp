//! A screen changed on purpose, as a release of a web application changes
//! one (roadmap package 9.1, hypothesis H-49).
//!
//! `POST /__control/screens` with `{"page": "line_item", "change": "reword"}`
//! changes the form of a line item, at `/orders/{order}/items/{item}`, until
//! the next reset or until it is told `"change": "none"`; `GET
//! /__control/screens` reports what each page shows now. Each change is one
//! a real release makes, and changes nothing the form sends or stores:
//!
//! - `reword`: the label "Quantity" reads "Qty", and the button "Save" reads
//!   "Save line item".
//! - `restyle`: every element gets class names and a style sheet, and every
//!   identifier is renamed, as a front-end build renames them; each label
//!   still names its field.
//! - `reorder`: the dimensions come first, then the quantity, then the
//!   description.
//! - `wrap`: the form is wrapped in a card whose header holds the heading
//!   "Line Item Details", which is no longer inside the form.

use serde::{Deserialize, Serialize};

/// A page the control interface can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangedPage {
    /// The form of one line item, `/orders/{order}/items/{item}`.
    LineItem,
}

/// How a page is changed, as a release would change it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenChange {
    /// The page as it was first released.
    #[default]
    None,
    /// A label and a button say other words.
    Reword,
    /// Class names and identifiers are new, and a style sheet is added.
    Restyle,
    /// The fields come in another order.
    Reorder,
    /// The form is wrapped in another element, which takes its heading.
    Wrap,
}

/// One change, as the control interface takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenRelease {
    pub page: ChangedPage,
    pub change: ScreenChange,
}

/// The suffix a restyled build gives every identifier.
const BUILD: &str = "7f3a";

/// The values of the form of one line item, already escaped for HTML.
pub(crate) struct ItemValues<'a> {
    pub(crate) order: &'a str,
    pub(crate) code: &'a str,
    pub(crate) description: &'a str,
    pub(crate) quantity: &'a str,
    pub(crate) length: &'a str,
    pub(crate) version: &'a str,
    pub(crate) code_lock: &'a str,
}

/// The body of the form of one line item, as `change` shows it, and its
/// script, which saves through the JSON interface.
pub(crate) fn item_form_body(
    change: ScreenChange,
    values: &ItemValues<'_>,
    script: &str,
) -> String {
    let restyled = change == ScreenChange::Restyle;
    let id = |name: &str| {
        if restyled {
            format!("li-{name}-{BUILD}")
        } else {
            name.to_owned()
        }
    };
    let class = |name: &str| {
        if restyled {
            format!(" class=\"{name}\"")
        } else {
            String::new()
        }
    };
    let (quantity_label, save_label) = if change == ScreenChange::Reword {
        ("Qty", "Save line item")
    } else {
        ("Quantity", "Save")
    };
    let ItemValues {
        order,
        code,
        description,
        quantity,
        length,
        version,
        code_lock,
    } = values;
    let row = class("ef-row");
    let input = class("ef-input");
    let code_row = format!(
        "<p{row}><label for=\"{field}\">Item code</label> \
         <input id=\"{field}\"{input} name=\"item_code\" value=\"{code}\" required{code_lock}></p>\n",
        field = id("item_code"),
    );
    let description_row = format!(
        "<p{row}><label for=\"{field}\">Description</label> \
         <input id=\"{field}\"{input} name=\"description\" value=\"{description}\" required></p>\n",
        field = id("description"),
    );
    let quantity_row = format!(
        "<p{row}><label for=\"{field}\">{quantity_label}</label> \
         <input id=\"{field}\"{input} name=\"quantity\" inputmode=\"numeric\" value=\"{quantity}\" required></p>\n",
        field = id("quantity"),
    );
    let dimensions = format!(
        "<fieldset{group}>\n<legend>Dimensions</legend>\n\
         <p{row}><label for=\"{field}\">Length</label> \
         <input id=\"{field}\"{input} name=\"length_in\" value=\"{length}\" aria-describedby=\"{hint}\" required> \
         <span id=\"{hint}\"{unit}>inches</span></p>\n</fieldset>\n",
        group = class("ef-group"),
        field = id("length"),
        hint = id("length-hint"),
        unit = class("ef-unit"),
    );
    let fields = if change == ScreenChange::Reorder {
        [code_row, dimensions, quantity_row, description_row].concat()
    } else {
        [code_row, description_row, quantity_row, dimensions].concat()
    };
    let heading = "<h2>Line Item Details</h2>\n";
    let inner_heading = if change == ScreenChange::Wrap {
        ""
    } else {
        heading
    };
    let form = format!(
        "<form id=\"{form_id}\"{form_class} data-order=\"{order}\" data-version=\"{version}\">\n\
         {inner_heading}{fields}\
         <p{row}><button type=\"submit\"{button}>{save_label}</button></p>\n\
         <p id=\"{status}\" role=\"status\"{note}></p>\n\
         <ul id=\"{problems}\" role=\"alert\"{note}></ul>\n\
         </form>\n",
        form_id = id("line-item"),
        form_class = class("ef-form"),
        button = class("ef-button ef-primary"),
        status = id("status"),
        problems = id("problems"),
        note = class("ef-note"),
    );
    let form = if change == ScreenChange::Wrap {
        format!(
            "<section class=\"card\" aria-labelledby=\"card-title\">\n\
             <header class=\"card-header\"><h2 id=\"card-title\">Line Item Details</h2></header>\n\
             <div class=\"card-body\">\n{form}</div>\n</section>\n"
        )
    } else {
        form
    };
    let style = if restyled {
        "<style>.ef-form{max-width:40rem}.ef-row{display:flex;gap:.5rem}\
         .ef-group{border:1px solid #ccc}.ef-primary{font-weight:bold}</style>\n"
    } else {
        ""
    };
    let script = if restyled {
        script
            .replace("\"line-item\"", &format!("\"{}\"", id("line-item")))
            .replace("\"status\"", &format!("\"{}\"", id("status")))
            .replace("\"problems\"", &format!("\"{}\"", id("problems")))
    } else {
        script.to_owned()
    };
    format!("{style}<h1>Order {order}</h1>\n{form}<script>\n{script}</script>")
}
