//! The `screens/` file kind for surfaces of the kind `web`: its schema, its
//! compilation into the screens of the compiled target, and the lint rules
//! over it.
//!
//! Locators live here and nowhere else. An operation names a screen, a
//! field, and an action; how each is found on the page is this file kind's
//! knowledge.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompileMode, CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind,
    JsonSchema, KindContext, Level, LintRule, Located, Place, ResolvedView, Stage,
};

use crate::codes::shown;
use crate::model::{
    Action, Cond, DownloadDecl, Field, Fingerprint, Handling, LocateBy, Locator, NativeDialog,
    PAYLOAD_FORMAT, ReadHow, Rows, Screen, ScreenSection, Segment, Variant,
};

/// The name of this pack, as its manifest gives it.
pub const PACK: &str = "web";
/// The name of the file kind.
pub const SCREEN_KIND: FileKindName = FileKindName::fixed("screen");
/// The surface kind this pack serves.
pub const WEB_SURFACE: SurfaceKind = SurfaceKind::fixed("web");

/// The controls this pack has a rule for. Each is declared in the manifest
/// and registered with the compiler, which refuses any other.
pub const CONTROLS: &[&str] = &[
    "text",
    "textarea",
    "number",
    "currency",
    "measurement",
    "select",
    "checkbox",
    "radio",
    "date",
    "combobox",
    "autocomplete",
    "datetime",
    "time",
    "slider",
    "file",
    "rich_text",
    "grid",
    "tree",
    "tabs",
    "accordion",
    "toggle",
];

/// The controls whose choices the field names under `options`, each code
/// with the label the page shows for it. A `radio`, a set of `tabs`, and an
/// `accordion` must: each is read as the code of the choice that is on. A
/// `combobox` and an `autocomplete` may; with none, the label is the value.
const CHOICES_REQUIRED: &[&str] = &["radio", "tabs", "accordion"];
const CHOICES_OPTIONAL: &[&str] = &["combobox", "autocomplete"];

/// The most a declared download may be, in bytes: a person's file, not a
/// disk's contents.
pub const MOST_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// A code of this pack's compiler: its lint rules, and what its file kind
/// and lowering refuse.
pub(crate) fn code(rule: &'static str) -> LintCode {
    // Every rule name in this crate is a lowercase name; a test holds that.
    LintCode::new(&format!("{PACK}.{rule}")).unwrap_or(LintCode::fixed("web.invalid"))
}

pub(crate) fn problem(
    rule: &'static str,
    stage: Stage,
    at: &Place,
    message: impl Into<String>,
    fix: &str,
) -> Diagnostic {
    Diagnostic::error(code(rule), stage, at, message, fix)
}

fn locator_schema(depth: u8) -> JsonSchema {
    let schema = JsonSchema::object()
        .optional("role", JsonSchema::text())
        .optional("name", JsonSchema::text())
        .optional("label", JsonSchema::text())
        .optional("placeholder", JsonSchema::text())
        .optional("text", JsonSchema::text())
        .optional("contains", JsonSchema::bool())
        .optional("test_id", JsonSchema::text())
        .optional("title", JsonSchema::text())
        .optional("heading", JsonSchema::int())
        .optional("css", JsonSchema::text());
    if depth == 0 {
        schema
    } else {
        schema.optional("within", locator_schema(depth - 1))
    }
}

fn condition_schema(depth: u8) -> JsonSchema {
    let schema = JsonSchema::object()
        .optional("visible", locator_schema(1))
        .optional("hidden", locator_schema(1))
        .optional("enabled", locator_schema(1))
        .optional(
            "text",
            JsonSchema::object()
                .required("locate", locator_schema(1))
                .optional("equals", JsonSchema::text())
                .optional("contains", JsonSchema::text()),
        );
    if depth == 0 {
        schema
    } else {
        schema
            .optional(
                "all",
                JsonSchema::non_empty_list(condition_schema(depth - 1)),
            )
            .optional(
                "any",
                JsonSchema::non_empty_list(condition_schema(depth - 1)),
            )
    }
}

fn fingerprint_schema() -> JsonSchema {
    JsonSchema::object()
        .required("section", locator_schema(1))
        .optional("near_text", JsonSchema::text())
}

fn locate_schema() -> JsonSchema {
    JsonSchema::non_empty_list(locator_schema(1))
}

/// The schema of one screen.
pub(crate) fn screen_schema() -> JsonSchema {
    let field = JsonSchema::object()
        .required("type", JsonSchema::field_type())
        .required("control", JsonSchema::control())
        .required("locate", locate_schema())
        .optional("fingerprint", fingerprint_schema())
        .optional("read", JsonSchema::words(&["value", "text", "checked"]))
        .optional("options", JsonSchema::map_of(JsonSchema::text()))
        .optional("column", JsonSchema::int());
    let action = JsonSchema::object()
        .required("locate", locate_schema())
        .optional("fingerprint", fingerprint_schema())
        .required("confirm", condition_schema(2))
        .optional("commits", JsonSchema::bool())
        .optional(
            "dialog",
            JsonSchema::object()
                .required("says", JsonSchema::text())
                .required("answer", JsonSchema::words(&["accept", "dismiss"])),
        )
        .optional(
            "download",
            JsonSchema::object()
                .required("most_bytes", JsonSchema::int())
                .optional("into", JsonSchema::name())
                .optional("size_into", JsonSchema::name()),
        );
    let rows = JsonSchema::object()
        .required("locate", locate_schema())
        .optional("fingerprint", fingerprint_schema())
        .optional("header", JsonSchema::non_empty_list(JsonSchema::text()))
        .optional("items", JsonSchema::words(crate::model::ITEM_ROLES))
        .required(
            "columns",
            JsonSchema::map_of(
                JsonSchema::object()
                    .optional("header", JsonSchema::text())
                    .required("type", JsonSchema::field_type()),
            ),
        );
    let variant = JsonSchema::object()
        .required(
            "when",
            JsonSchema::object().required("visible", locator_schema(1)),
        )
        .required(
            "handling",
            JsonSchema::words(&["absent", "stop", "confirm", "dismiss"]),
        )
        .optional("with", JsonSchema::name());
    JsonSchema::object()
        .required("screen", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required(
            "reach",
            JsonSchema::object().required("url", JsonSchema::text()),
        )
        .optional("frame", JsonSchema::non_empty_list(locator_schema(1)))
        .required("ready", condition_schema(2))
        .optional("fields", JsonSchema::map_of(field))
        .optional("actions", JsonSchema::map_of(action))
        .optional("rows", rows)
        .optional("variants", JsonSchema::map_of(variant))
}

/// Whether a character may stand as written in a segment of an address.
pub(crate) fn unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}

/// A name a placeholder may have: an input's name.
pub(crate) fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Takes the path of a screen's address apart. A placeholder is one whole
/// segment; a query and a fragment are not accepted.
pub(crate) fn parse_reach(text: &str) -> Result<Vec<Segment>, String> {
    let Some(rest) = text.strip_prefix('/') else {
        return Err("it does not start with `/`".to_owned());
    };
    let mut segments = Vec::new();
    if rest.is_empty() {
        return Ok(segments);
    }
    for part in rest.split('/') {
        if let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            if !is_name(name) {
                return Err(format!(
                    "the placeholder `{}` is not a lowercase name",
                    shown(part)
                ));
            }
            segments.push(Segment::Hole(name.to_owned()));
        } else if part.is_empty() || part == "." || part == ".." || !part.chars().all(unreserved) {
            return Err(format!(
                "the segment `{}` is not letters, digits, `-`, `.`, `_`, or `~`, or one whole placeholder",
                shown(part)
            ));
        } else {
            segments.push(Segment::Literal(part.to_owned()));
        }
    }
    Ok(segments)
}

/// Reads documents into the model, collecting what is wrong.
struct Reader {
    problems: Diagnostics,
}

impl Reader {
    fn refuse(&mut self, rule: &'static str, at: &Place, message: String, fix: &str) {
        self.problems
            .push(problem(rule, Stage::Resolve, at, message, fix));
    }

    fn locator(&mut self, doc: &Located<Doc>) -> Option<Locator> {
        let text = |key: &str| doc.text(key).map(str::to_owned);
        let mut kinds: Vec<LocateBy> = Vec::new();
        match (text("role"), text("name")) {
            (Some(role), Some(name)) => kinds.push(LocateBy::Role { role, name }),
            (None, None) => {}
            _ => {
                self.refuse(
                    "locator_invalid",
                    &doc.at,
                    "A locator by role states both `role` and `name`.".to_owned(),
                    "Write both; `name: \"\"` matches an element that has no accessible name.",
                );
                return None;
            }
        }
        if let Some(label) = text("label") {
            kinds.push(LocateBy::Label(label));
        }
        if let Some(placeholder) = text("placeholder") {
            kinds.push(LocateBy::Placeholder(placeholder));
        }
        let contains = doc.get("contains").and_then(Located::as_bool);
        let shown_text = text("text");
        let stray_contains = contains.is_some() && shown_text.is_none();
        if let Some(shown_text) = shown_text {
            kinds.push(LocateBy::Text {
                text: shown_text,
                contains: contains.unwrap_or(false),
            });
        }
        if let Some(id) = text("test_id") {
            kinds.push(LocateBy::TestId(id));
        }
        if let Some(title) = text("title") {
            kinds.push(LocateBy::Title(title));
        }
        if let Some(level) = doc.get("heading") {
            match level.as_int().and_then(|level| u32::try_from(level).ok()) {
                Some(level @ 1..=6) => kinds.push(LocateBy::Heading(level)),
                _ => {
                    self.refuse(
                        "locator_invalid",
                        &doc.at,
                        "A locator by `heading` states the heading's level, from 1 to 6."
                            .to_owned(),
                        "Write the level the page gives the heading.",
                    );
                    return None;
                }
            }
        }
        if let Some(selector) = text("css") {
            kinds.push(LocateBy::Css(selector));
        }
        if kinds.len() != 1 || stray_contains {
            self.refuse(
                "locator_invalid",
                &doc.at,
                "A locator matches in exactly one way: `role` with `name`, `label`, `placeholder`, `text`, `test_id`, `title`, `heading`, or `css`; `contains` goes with `text` only.".to_owned(),
                "Write one of them; add a second locator to the list for a second way.",
            );
            return None;
        }
        let within = match doc.get("within") {
            Some(scope) => Some(Box::new(self.locator(scope)?)),
            None => None,
        };
        Some(Locator {
            by: kinds.remove(0),
            within,
        })
    }

    fn locators(&mut self, doc: &Located<Doc>) -> Option<Vec<Locator>> {
        let list = doc.get("locate")?;
        let found: Vec<Locator> = list
            .items()
            .iter()
            .filter_map(|item| self.locator(item))
            .collect();
        (found.len() == list.items().len()).then_some(found)
    }

    /// The fingerprint of a field, an action, or a table. A locator that
    /// has a `within` of its own cannot also be held to a section: the two
    /// scopes cannot be checked as one.
    fn fingerprint(
        &mut self,
        doc: &Located<Doc>,
        locators: &[Locator],
    ) -> Option<Option<Fingerprint>> {
        let Some(stated) = doc.get("fingerprint") else {
            return Some(None);
        };
        let section = self.locator(stated.get("section")?)?;
        if locators.iter().any(|locator| locator.within.is_some()) {
            self.refuse(
                "within_and_section",
                &stated.at,
                "A locator with `within` is under a fingerprint with a `section`; the element can be held to one of them only.".to_owned(),
                "Remove `within` from the locator; the fingerprint's `section` already says where the element is.",
            );
            return None;
        }
        Some(Some(Fingerprint {
            section,
            near_text: stated.text("near_text").map(str::to_owned),
            frame: None,
        }))
    }

    fn condition(&mut self, doc: &Located<Doc>) -> Option<Cond> {
        let mut found = Vec::new();
        for entry in doc.entries() {
            let value = &entry.value;
            let condition = match entry.key.value.as_str() {
                "visible" => Cond::Visible(self.locator(value)?),
                "hidden" => Cond::Hidden(self.locator(value)?),
                "enabled" => Cond::Enabled(self.locator(value)?),
                "text" => {
                    let locate = self.locator(value.get("locate")?)?;
                    match (value.text("equals"), value.text("contains")) {
                        (Some(text), None) => Cond::Text {
                            locate,
                            text: text.to_owned(),
                            contains: false,
                        },
                        (None, Some(text)) => Cond::Text {
                            locate,
                            text: text.to_owned(),
                            contains: true,
                        },
                        _ => {
                            self.refuse(
                                "condition_invalid",
                                &value.at,
                                "A `text` condition states exactly one of `equals` and `contains`."
                                    .to_owned(),
                                "Write one of them.",
                            );
                            return None;
                        }
                    }
                }
                "all" | "any" => {
                    let of: Vec<Cond> = value
                        .items()
                        .iter()
                        .filter_map(|item| self.condition(item))
                        .collect();
                    if of.len() != value.items().len() {
                        return None;
                    }
                    if entry.key.value == "all" {
                        Cond::All(of)
                    } else {
                        Cond::Any(of)
                    }
                }
                _ => return None,
            };
            found.push(condition);
        }
        if found.len() != 1 {
            self.refuse(
                "condition_invalid",
                &doc.at,
                "A condition is exactly one of `visible`, `hidden`, `enabled`, `text`, `all`, and `any`.".to_owned(),
                "Write one; join several with `all` or `any`.",
            );
            return None;
        }
        found.pop()
    }

    fn field(&mut self, doc: &Located<Doc>) -> Option<Field> {
        let control = doc.text("control")?.to_owned();
        let locate = self.locators(doc)?;
        let fingerprint = self.fingerprint(doc, &locate)?;
        let read = match (doc.text("read"), control.as_str()) {
            (Some("value"), _) => ReadHow::Value,
            (Some("text"), _) => ReadHow::Text,
            (Some("checked"), _) | (None, "checkbox" | "toggle") => ReadHow::Checked,
            _ => ReadHow::Value,
        };
        if (read == ReadHow::Checked) != matches!(control.as_str(), "checkbox" | "toggle") {
            self.refuse(
                "read_not_fitting",
                &doc.at,
                format!(
                    "The control `{}` cannot be read as the field says.",
                    shown(&control)
                ),
                "A `checkbox` and a `toggle` are read as `checked`, and every other control as `value` or `text`.",
            );
            return None;
        }
        let mut options = BTreeMap::new();
        for entry in doc.get("options").map(Located::entries).unwrap_or_default() {
            if let Some(label) = entry.value.as_text() {
                options.insert(entry.key.value.clone(), label.to_owned());
            }
        }
        let required = CHOICES_REQUIRED.contains(&control.as_str());
        let allowed = required || CHOICES_OPTIONAL.contains(&control.as_str());
        if (required && options.is_empty()) || (!allowed && !options.is_empty()) {
            self.refuse(
                "options_not_fitting",
                &doc.at,
                if required {
                    format!(
                        "A `{}` field names its choices under `options`, each code with the label the page shows for it.",
                        shown(&control)
                    )
                } else {
                    format!(
                        "The control `{}` has no `options`; only a `radio`, `tabs`, `accordion`, `combobox`, or `autocomplete` takes them.",
                        shown(&control)
                    )
                },
                "Write `options:` for a control that has choices, and nowhere else.",
            );
            return None;
        }
        let column = match doc.get("column").and_then(Located::as_int) {
            Some(place) => match u32::try_from(place) {
                Ok(place) => Some(place),
                Err(_) => {
                    self.refuse(
                        "column_not_fitting",
                        &doc.at,
                        "A `column` is the place of a cell in its row, counted from zero."
                            .to_owned(),
                        "Write a whole number of zero or more.",
                    );
                    return None;
                }
            },
            None => None,
        };
        if (control == "grid") != column.is_some() {
            self.refuse(
                "column_not_fitting",
                &doc.at,
                if control == "grid" {
                    "A `grid` field names the `column` of its cell: its place among the cells of a row, counted from zero.".to_owned()
                } else {
                    format!(
                        "The control `{}` has no `column`; only a `grid` takes one.",
                        shown(&control)
                    )
                },
                "Write `column:` for a grid cell, and nowhere else.",
            );
            return None;
        }
        Some(Field {
            control,
            locate,
            fingerprint,
            read,
            options,
            column,
        })
    }

    fn action(&mut self, doc: &Located<Doc>) -> Option<Action> {
        let locate = self.locators(doc)?;
        let fingerprint = self.fingerprint(doc, &locate)?;
        let dialog = doc.get("dialog").and_then(|stated| {
            Some(NativeDialog {
                says: stated.text("says")?.to_owned(),
                accept: stated.text("answer") == Some("accept"),
            })
        });
        let download = match doc.get("download") {
            Some(stated) => {
                let most = stated
                    .get("most_bytes")
                    .and_then(Located::as_int)
                    .and_then(|most| u64::try_from(most).ok())
                    .filter(|most| (1..=MOST_DOWNLOAD_BYTES).contains(most));
                let Some(most_bytes) = most else {
                    self.refuse(
                        "download_invalid",
                        &stated.at,
                        format!(
                            "The most a download may hold is between 1 and {MOST_DOWNLOAD_BYTES} bytes."
                        ),
                        "Write `most_bytes:` with the size the file may be, in bytes.",
                    );
                    return None;
                };
                let into = stated.text("into").map(str::to_owned);
                let size_into = stated.text("size_into").map(str::to_owned);
                if into.is_some() && into == size_into {
                    self.refuse(
                        "download_invalid",
                        &stated.at,
                        "The download's name and its size are given to the same output.".to_owned(),
                        "Name one output in `into:` for the file's name and another in `size_into:` for its size.",
                    );
                    return None;
                }
                Some(DownloadDecl {
                    most_bytes,
                    into,
                    size_into,
                })
            }
            None => None,
        };
        Some(Action {
            locate,
            fingerprint,
            confirm: self.condition(doc.get("confirm")?)?,
            commits: doc.get("commits").and_then(Located::as_bool) == Some(true),
            dialog,
            download,
        })
    }

    fn rows(&mut self, doc: &Located<Doc>) -> Option<Rows> {
        let locate = self.locators(doc)?;
        let fingerprint = self.fingerprint(doc, &locate)?;
        // Items: each one row of one cell, its text; no heading at all
        // (decision 0999).
        if let Some(role) = doc.text("items") {
            let entries = doc.get("columns")?.entries();
            let stray = doc.get("header").is_some()
                || entries.len() != 1
                || entries
                    .iter()
                    .any(|entry| entry.value.get("header").is_some());
            if stray {
                self.refuse(
                    "column_unknown",
                    &doc.at,
                    "Rows read as `items` have no `header`, and exactly one column, with no `header` of its own: the text of each item.".to_owned(),
                    "Remove `header`, and keep one column under `columns`.",
                );
                return None;
            }
            return Some(Rows {
                locate,
                fingerprint,
                header: Vec::new(),
                columns: entries
                    .iter()
                    .map(|entry| (entry.key.value.clone(), 0))
                    .collect(),
                items: Some(role.to_owned()),
            });
        }
        let Some(listed) = doc.get("header") else {
            self.refuse(
                "column_unknown",
                &doc.at,
                "A table's rows state its `header`: the text of every column's heading.".to_owned(),
                "Write the headings as the page shows them, or `items:` for a list.",
            );
            return None;
        };
        let header: Vec<String> = listed
            .items()
            .iter()
            .filter_map(|item| item.as_text().map(str::to_owned))
            .collect();
        let mut columns = BTreeMap::new();
        for entry in doc.get("columns")?.entries() {
            let heading = entry.value.text("header").unwrap_or_default();
            let position = header.iter().position(|known| known == heading);
            let unique = header.iter().filter(|known| *known == heading).count() == 1;
            match position.and_then(|position| u32::try_from(position).ok()) {
                Some(position) if unique => {
                    columns.insert(entry.key.value.clone(), position);
                }
                _ => {
                    self.refuse(
                        "column_unknown",
                        &entry.value.at,
                        format!(
                            "The column `{}` names the heading `{}`, which the table's `header` does not list exactly once.",
                            shown(&entry.key.value),
                            shown(heading)
                        ),
                        "Write the heading as `header` lists it.",
                    );
                }
            }
        }
        Some(Rows {
            locate,
            fingerprint,
            header,
            columns,
            items: None,
        })
    }

    fn screen(&mut self, doc: &Located<Doc>, surface: &str) -> Option<Screen> {
        let before = self.problems.len();
        let reach_doc = doc.get("reach")?.get("url")?;
        let reach = match parse_reach(reach_doc.as_text()?) {
            Ok(reach) => Some(reach),
            Err(why) => {
                self.refuse(
                    "reach_invalid",
                    &reach_doc.at,
                    format!("The address of the screen cannot be used: {why}."),
                    "Write a path such as `/orders/{order_id}`, with each placeholder one whole segment, and no query.",
                );
                None
            }
        };
        let ready = self.condition(doc.get("ready")?);
        let mut fields = BTreeMap::new();
        for entry in doc.get("fields").map(Located::entries).unwrap_or_default() {
            if let Some(field) = self.field(&entry.value) {
                fields.insert(entry.key.value.clone(), field);
            }
        }
        let mut actions = BTreeMap::new();
        for entry in doc.get("actions").map(Located::entries).unwrap_or_default() {
            if let Some(action) = self.action(&entry.value) {
                actions.insert(entry.key.value.clone(), action);
            }
        }
        let rows = match doc.get("rows") {
            Some(rows) => self.rows(rows),
            None => None,
        };
        let mut variants = BTreeMap::new();
        for entry in doc
            .get("variants")
            .map(Located::entries)
            .unwrap_or_default()
        {
            let when = entry
                .value
                .get("when")
                .and_then(|when| when.get("visible"))
                .and_then(|locator| self.locator(locator));
            let handling = match entry.value.text("handling") {
                Some("absent") => Some(Handling::Absent),
                Some("stop") => Some(Handling::Stop),
                Some("confirm") => Some(Handling::Confirm),
                Some("dismiss") => Some(Handling::Dismiss),
                _ => None,
            };
            let with = entry.value.text("with").map(str::to_owned);
            let answers = matches!(handling, Some(Handling::Confirm | Handling::Dismiss));
            match (&with, answers) {
                (Some(name), true) if !actions.contains_key(name) => {
                    self.refuse(
                        "variant_with_unknown",
                        &entry.value.at,
                        format!(
                            "The variant `{}` is answered with the action `{}`, which the screen does not declare.",
                            shown(&entry.key.value),
                            shown(name)
                        ),
                        "Declare the action on the screen, or name one it has.",
                    );
                }
                (None, true) => {
                    self.refuse(
                        "variant_without_answer",
                        &entry.value.at,
                        format!(
                            "The variant `{}` is to be confirmed or dismissed and names no action to do it with.",
                            shown(&entry.key.value)
                        ),
                        "Add `with:` and the name of the action that answers it.",
                    );
                }
                (Some(_), false) => {
                    self.refuse(
                        "variant_with_unused",
                        &entry.value.at,
                        format!(
                            "The variant `{}` names an action to answer it with, and its handling answers nothing.",
                            shown(&entry.key.value)
                        ),
                        "Remove `with:`, or make the handling `confirm` or `dismiss`.",
                    );
                }
                _ => {}
            }
            if let (Some(when), Some(handling)) = (when, handling) {
                variants.insert(
                    entry.key.value.clone(),
                    Variant {
                        when,
                        handling,
                        with,
                    },
                );
            }
        }
        // The frame the screen is in, outermost first: each frame is
        // looked for inside the one before it.
        let mut frame: Option<Locator> = None;
        for stated in doc.get("frame").map(Located::items).unwrap_or_default() {
            if let Some(mut next) = self.locator(stated) {
                if let Some(outer) = &frame {
                    next.inside(outer);
                }
                frame = Some(next);
            }
        }
        if self.problems.len() != before {
            return None;
        }
        let mut screen = Screen {
            surface: surface.to_owned(),
            reach: reach?,
            ready: ready?,
            fields,
            actions,
            rows,
            variants,
        };
        if let Some(frame) = &frame {
            screen.inside(frame);
        }
        Some(screen)
    }
}

/// The `screens/` file kind, for `web` surfaces.
pub(crate) struct ScreenKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl ScreenKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: screen_schema(),
            kinds: vec![WEB_SURFACE],
        }
    }
}

impl FileKind for ScreenKind {
    fn name(&self) -> FileKindName {
        SCREEN_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "screens"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    /// A screen that declares any action describes a write: an action is a
    /// click on the system, and whether it saves is the word of whoever
    /// wrote it. The drafter writes none (decision 0144).
    fn writes(&self, doc: &Located<Doc>) -> bool {
        doc.get("actions")
            .is_some_and(|actions| !actions.entries().is_empty())
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("screen")
            .and_then(|name| {
                name.as_text().map(|text| {
                    Located::new(
                        DefinedName {
                            name: text.to_owned(),
                        },
                        name.at.clone(),
                    )
                })
            })
            .into_iter()
            .collect()
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        // A screen holds locators and conditions, and no expression: values
        // arrive from a binding, into a field or a placeholder.
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut section = ScreenSection::new();
        let mut reader = Reader {
            problems: Diagnostics::new(),
        };
        for doc in docs {
            let name = doc.text("screen").unwrap_or_default();
            let Some(surface) = cx.surface_of(doc) else {
                continue;
            };
            if let Some(screen) = reader.screen(doc, surface.name.as_str()) {
                section.insert(name.to_owned(), screen);
            }
        }
        if !reader.problems.is_empty() {
            return Err(reader.problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: PAYLOAD_FORMAT,
                payload,
            })
            .map_err(|failure| {
                Diagnostics::from(problem(
                    "not_encodable",
                    Stage::Lower,
                    &docs.first().map_or_else(
                        || Place::start_of(&a2m_files::RelPath::target_file()),
                        |doc| doc.at.clone(),
                    ),
                    failure.message().to_owned(),
                    "Report this to the web pack's author.",
                ))
            })
    }
}

/// Every part of every screen that has locators: each field, each action,
/// and the table.
fn located_parts<'a>(view: &ResolvedView<'a>) -> Vec<(&'a str, &'a Located<Doc>)> {
    let mut parts = Vec::new();
    for doc in view.entries(&SCREEN_KIND, &WEB_SURFACE) {
        for key in ["fields", "actions"] {
            for entry in doc.get(key).map(Located::entries).unwrap_or_default() {
                parts.push((entry.key.value.as_str(), &entry.value));
            }
        }
        if let Some(rows) = doc.get("rows") {
            parts.push(("rows", rows));
        }
    }
    parts
}

fn is_css(locator: &Located<Doc>) -> bool {
    locator.get("css").is_some()
}

/// `web.css_not_last`.
pub(crate) struct CssNotLast;

impl LintRule for CssNotLast {
    fn code(&self) -> LintCode {
        code("css_not_last")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "A `css` locator is written only last in a list of locators."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for (name, part) in located_parts(view) {
            let list = part.get("locate").map(Located::items).unwrap_or_default();
            for locator in list.iter().rev().skip(1).filter(|l| is_css(l)) {
                out.push(problem(
                    "css_not_last",
                    Stage::Lint,
                    &locator.at,
                    format!(
                        "A `css` locator of `{}` is followed by another locator.",
                        shown(name)
                    ),
                    "Move it to the end of the list: a selector is the last resort.",
                ));
            }
        }
        out
    }
}

/// `web.css_used`.
pub(crate) struct CssUsed;

impl LintRule for CssUsed {
    fn code(&self) -> LintCode {
        code("css_used")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Warning
    }

    fn summary(&self) -> &str {
        "No field, action, or table is located by a `css` selector."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for (name, part) in located_parts(view) {
            let list = part.get("locate").map(Located::items).unwrap_or_default();
            for locator in list.iter().filter(|l| is_css(l)) {
                out.push(problem(
                    "css_used",
                    Stage::Lint,
                    &locator.at,
                    format!("`{}` is located by a `css` selector.", shown(name)),
                    "Locate it by role and name, by label, or by a test identifier where the page has one.",
                ));
            }
        }
        out
    }
}

/// `web.no_fingerprint`.
pub(crate) struct NoFingerprint;

impl LintRule for NoFingerprint {
    fn code(&self) -> LintCode {
        code("no_fingerprint")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "Every field, action, and table of a screen states a fingerprint."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        located_parts(view)
            .into_iter()
            .filter(|(_, part)| part.get("fingerprint").is_none())
            .map(|(name, part)| {
                problem(
                    "no_fingerprint",
                    Stage::Lint,
                    &part.at,
                    format!("`{}` states no fingerprint.", shown(name)),
                    "Add `fingerprint:` with the `section` the element is inside, and `near_text` where the section shows a fixed text.",
                )
            })
            .collect()
    }
}

/// The surface's profile, when the surface names one that exists and is not
/// `auto`. A name that exists nowhere is `web.profile_unknown`'s.
fn platform_profile_of(
    view: &ResolvedView<'_>,
    surface: &str,
) -> Option<&'static crate::profiles::PlatformProfile> {
    let named = view
        .target()
        .surface(surface)?
        .doc
        .get("profile")?
        .as_text()?;
    crate::profiles::named(named)
}

/// The identifiers a selector is built on: `#name`, and `[id=name]`.
fn ids_of(selector: &str) -> Vec<String> {
    static IDS: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let Some(pattern) = IDS
        .get_or_init(|| {
            regex::Regex::new(r#"#([A-Za-z_][\w-]*)|\[id\s*[~|^$*]?=\s*["']?([^"'\]]+)"#).ok()
        })
        .as_ref()
    else {
        return Vec::new();
    };
    pattern
        .captures_iter(selector)
        .filter_map(|found| found.get(1).or_else(|| found.get(2)))
        .map(|found| found.as_str().to_owned())
        .collect()
}

/// `web.generated_id`.
pub(crate) struct GeneratedId;

impl LintRule for GeneratedId {
    fn code(&self) -> LintCode {
        code("generated_id")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "No locator is built on an identifier that the surface's platform profile says the platform makes."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for screen in view.entries(&SCREEN_KIND, &WEB_SURFACE) {
            let Some(profile) = screen
                .text("surface")
                .and_then(|surface| platform_profile_of(view, surface))
            else {
                continue;
            };
            for key in ["fields", "actions"] {
                for entry in screen.get(key).map(Located::entries).unwrap_or_default() {
                    for locator in entry
                        .value
                        .get("locate")
                        .map(Located::items)
                        .unwrap_or_default()
                    {
                        Self::look(profile, &entry.key.value, locator, &mut out);
                    }
                }
            }
        }
        out
    }
}

impl GeneratedId {
    fn look(
        profile: &crate::profiles::PlatformProfile,
        part: &str,
        locator: &Located<Doc>,
        out: &mut Vec<Diagnostic>,
    ) {
        let mut ids = locator
            .get("css")
            .and_then(Located::as_text)
            .map(ids_of)
            .unwrap_or_default();
        ids.extend(
            locator
                .get("test_id")
                .and_then(Located::as_text)
                .map(str::to_owned),
        );
        for id in ids {
            if profile.rejects_id(&id) {
                out.push(problem(
                    "generated_id",
                    Stage::Lint,
                    &locator.at,
                    format!(
                        "A locator of `{}` is built on the identifier `{}`, which the profile `{}` says the platform makes.",
                        shown(part),
                        shown(&id),
                        profile.name
                    ),
                    "Locate it by its label, its role and name, or its text; an identifier the platform makes changes when the page does.",
                ));
            }
        }
        if let Some(inside) = locator.get("within") {
            Self::look(profile, part, inside, out);
        }
    }
}

/// `web.profile_unknown`.
pub(crate) struct PlatformProfileUnknown;

impl LintRule for PlatformProfileUnknown {
    fn code(&self) -> LintCode {
        code("profile_unknown")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "A surface's `profile` is the name of a platform profile, or `auto`."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for surface in view.target().surfaces() {
            if surface.kind != &WEB_SURFACE {
                continue;
            }
            let Some(named) = surface.doc.get("profile") else {
                continue;
            };
            let text = named.as_text().unwrap_or("");
            if text != "auto" && crate::profiles::named(text).is_none() {
                out.push(problem(
                    "profile_unknown",
                    Stage::Lint,
                    &named.at,
                    format!(
                        "The surface `{}` names the profile `{}`, which is not one of {}.",
                        surface.name,
                        shown(text),
                        crate::profiles::names().join(", ")
                    ),
                    "Name one of the profiles, or `auto` to have the probe name it.",
                ));
            }
        }
        out
    }
}

/// `web.profile_readonly`: a write bound through a screen whose surface's
/// profile has the tier `read_only` or `unsupported`. It holds whatever
/// mode binds the screen, since a mode that acts on the page acts on
/// controls the profile says are not checked for writing.
pub(crate) struct PlatformProfileReadonly;

impl LintRule for PlatformProfileReadonly {
    fn code(&self) -> LintCode {
        code("profile_readonly")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "No write is bound through a screen of a surface whose platform profile's tier is `read_only` or `unsupported`."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let screens = view.entries(&SCREEN_KIND, &WEB_SURFACE);
        for operation in view.operations() {
            if operation.effect() == a2m_core::EffectClass::Read {
                continue;
            }
            for binding in operation
                .doc()
                .get("bindings")
                .map(Located::items)
                .unwrap_or_default()
            {
                let Some(named) = binding.get("screen") else {
                    continue;
                };
                let Some(name) = named.as_text() else {
                    continue;
                };
                let Some((surface, profile)) = screens
                    .iter()
                    .find(|screen| screen.text("screen") == Some(name))
                    .and_then(|screen| screen.text("surface"))
                    .and_then(|surface| Some((surface, platform_profile_of(view, surface)?)))
                else {
                    continue;
                };
                if profile.may_write() {
                    continue;
                }
                out.push(problem(
                    "profile_readonly",
                    Stage::Lint,
                    &named.at,
                    format!(
                        "The operation `{}` writes through the screen `{}`, on the surface `{}`, whose profile `{}` has the tier `{}`: its controls are not checked for writing.",
                        operation.name(),
                        shown(name),
                        shown(surface),
                        profile.name,
                        profile.support_tier
                    ),
                    "Bind the write to a request instead (`page_http`, or `http` on the service behind the page), or keep the operation a read; the profile says this platform's controls may be read and not written.",
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_plain_segments_and_whole_placeholders() {
        assert_eq!(parse_reach("/"), Ok(Vec::new()));
        assert_eq!(
            parse_reach("/orders/{order_id}/items"),
            Ok(vec![
                Segment::Literal("orders".to_owned()),
                Segment::Hole("order_id".to_owned()),
                Segment::Literal("items".to_owned()),
            ])
        );
        for refused in [
            "orders",
            "/orders/",
            "/orders/../x",
            "/orders?x=1",
            "/orders/a{order_id}",
            "/orders/{Order}",
            "/orders/a b",
            "//other.example.com/x",
            "/orders#top",
        ] {
            assert!(parse_reach(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn every_rule_name_is_a_lint_code_of_this_pack() {
        for rule in [
            "css_not_last",
            "css_used",
            "no_fingerprint",
            "locator_invalid",
            "condition_invalid",
            "within_and_section",
            "read_not_fitting",
            "column_unknown",
            "reach_invalid",
            "not_encodable",
            "unknown_screen",
            "unknown_field",
            "unknown_action",
            "placeholder_unbound",
            "position_type",
            "value_type",
            "action_missing",
            "read_acts",
            "read_back_never_absent",
        ] {
            assert_eq!(code(rule).as_str(), format!("web.{rule}"));
        }
        for control in CONTROLS {
            assert!(a2m_core::ControlName::new(control).is_ok());
        }
    }
}
