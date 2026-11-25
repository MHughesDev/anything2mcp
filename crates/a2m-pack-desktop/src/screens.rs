//! The `screens/` file kind for surfaces of the kind `desktop`: its schema,
//! and its compilation into the screens of the compiled target.
//!
//! Locators live here and nowhere else (rule 8). A desktop locator is a
//! role and an accessible name, both exact: no text near it, no selector,
//! no position on the screen. A screen may declare actions, each with the
//! name of the element's action it does, the confirmation the window shows
//! once it is done, and whether it saves; and variants, the states the
//! window may show instead, each the application's refusal (decision
//! 0218). A screen file with an action that saves describes a write.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Place, Stage,
};

use crate::codes::shown;
use crate::model::{
    Action, Cond, Field, Locator, PAYLOAD_FORMAT, Rows, Screen, ScreenSection, TitleHole, Variant,
    WindowMatch,
};
use crate::roles;

/// The name of this pack, as its manifest gives it.
pub const PACK: &str = "desktop";
/// The name of the file kind.
pub const SCREEN_KIND: FileKindName = FileKindName::fixed("screen");
/// The surface kind this pack serves.
pub const DESKTOP_SURFACE: SurfaceKind = SurfaceKind::fixed("desktop");

/// The controls this pack has a rule for: a value read as text. Each is
/// declared in the manifest and registered with the compiler, which
/// refuses any other.
pub const CONTROLS: &[&str] = &["desktop_text"];

/// A code of this pack's compiler.
pub(crate) fn code(rule: &'static str) -> LintCode {
    // Every rule name in this crate is a lowercase name; a test holds that.
    LintCode::new(&format!("{PACK}.{rule}")).unwrap_or(LintCode::fixed("desktop.invalid"))
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

fn locator_schema() -> JsonSchema {
    JsonSchema::object()
        .required("role", JsonSchema::text())
        .required("name", JsonSchema::text())
}

fn condition_schema(depth: u8) -> JsonSchema {
    let schema = JsonSchema::object()
        .optional("visible", locator_schema())
        .optional(
            "text",
            JsonSchema::object()
                .required("locate", locator_schema())
                .optional("equals", JsonSchema::text())
                .optional("contains", JsonSchema::text()),
        );
    if depth == 0 {
        schema
    } else {
        schema.optional(
            "all",
            JsonSchema::non_empty_list(condition_schema(depth - 1)),
        )
    }
}

fn locate_schema() -> JsonSchema {
    JsonSchema::non_empty_list(locator_schema())
}

/// The schema of one screen.
pub(crate) fn screen_schema() -> JsonSchema {
    let field = JsonSchema::object()
        .required("type", JsonSchema::field_type())
        .required("control", JsonSchema::control())
        .required("locate", locate_schema());
    let rows = JsonSchema::object()
        .optional("locate", locate_schema())
        .required("header", JsonSchema::non_empty_list(JsonSchema::text()))
        .required(
            "columns",
            JsonSchema::map_of(
                JsonSchema::object()
                    .required("header", JsonSchema::text())
                    .required("type", JsonSchema::field_type()),
            ),
        );
    JsonSchema::object()
        .required("screen", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required(
            "reach",
            JsonSchema::object().required(
                "window",
                JsonSchema::object()
                    .required("role", JsonSchema::words(roles::WINDOW_ROLES))
                    .required("title", JsonSchema::text()),
            ),
        )
        .required("ready", condition_schema(2))
        .optional("fields", JsonSchema::map_of(field))
        .optional("rows", rows)
        .optional("actions", JsonSchema::map_of(action_schema()))
        .optional(
            "variants",
            JsonSchema::map_of(
                JsonSchema::object()
                    .required("when", condition_schema(1))
                    .required("handling", JsonSchema::words(&["stop"])),
            ),
        )
}

/// One action: where it is done, which of the element's actions is done
/// (`click` unless stated, or the one the element binds to a `key` of the
/// pack's closed list), what the window shows once it is done, and whether
/// it saves.
fn action_schema() -> JsonSchema {
    JsonSchema::object()
        .required("locate", locate_schema())
        .optional("perform", JsonSchema::name())
        .optional("key", JsonSchema::words(crate::keys::KEYS))
        .required("confirm", condition_schema(1))
        .optional("commits", JsonSchema::bool())
}

/// The name of the element's action an action does when it states none.
pub(crate) const DEFAULT_PERFORM: &str = "click";

/// Whether a screen document declares an action that saves.
fn declares_commit(doc: &Located<Doc>) -> bool {
    doc.get("actions").is_some_and(|actions| {
        actions
            .entries()
            .iter()
            .any(|entry| entry.value.get("commits").and_then(Located::as_bool) == Some(true))
    })
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
        let role = doc.text("role")?.to_owned();
        let name = doc.text("name")?.to_owned();
        if !roles::known(&role) {
            self.refuse(
                "role_unknown",
                &doc.at,
                format!(
                    "The role `{}` is not one the desktop pack locates by.",
                    shown(&role)
                ),
                "Write one of the pack's role words, such as `entry`, `label`, `table`, or `push_button`.",
            );
            return None;
        }
        if name.trim().is_empty() {
            self.refuse(
                "unnamed",
                &doc.at,
                "A desktop locator names the element; an element with no accessible name cannot be told apart from others of its role.".to_owned(),
                "Name the element as the application names it. A control the application gives no name is not read in this version.",
            );
            return None;
        }
        Some(Locator { role, name })
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

    fn condition(&mut self, doc: &Located<Doc>) -> Option<Cond> {
        let mut found = Vec::new();
        for entry in doc.entries() {
            let value = &entry.value;
            let condition = match entry.key.value.as_str() {
                "visible" => Cond::Visible(self.locator(value)?),
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
                "all" => {
                    let of: Vec<Cond> = value
                        .items()
                        .iter()
                        .filter_map(|item| self.condition(item))
                        .collect();
                    if of.len() != value.items().len() {
                        return None;
                    }
                    Cond::All(of)
                }
                _ => return None,
            };
            found.push(condition);
        }
        if found.len() != 1 {
            self.refuse(
                "condition_invalid",
                &doc.at,
                "A condition is exactly one of `visible`, `text`, and `all`.".to_owned(),
                "Write one; join several with `all`.",
            );
            return None;
        }
        found.pop()
    }

    /// A table: located by its locators, or, with none, as the one table
    /// of the window whose headings are its `header` (decision 0462).
    fn rows(&mut self, doc: &Located<Doc>) -> Option<Rows> {
        let locate = if doc.get("locate").is_some() {
            self.locators(doc)?
        } else {
            Vec::new()
        };
        let header: Vec<String> = doc
            .get("header")?
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
                _ => self.refuse(
                    "column_unknown",
                    &entry.value.at,
                    format!(
                        "The column `{}` names the heading `{}`, which the table's `header` does not list exactly once.",
                        shown(&entry.key.value),
                        shown(heading)
                    ),
                    "Write the heading as `header` lists it.",
                ),
            }
        }
        Some(Rows {
            locate,
            header,
            columns,
        })
    }

    fn screen(&mut self, doc: &Located<Doc>, surface: &str) -> Option<Screen> {
        let before = self.problems.len();
        let window = doc.get("reach")?.get("window")?;
        let title = window.text("title")?.to_owned();
        if title.trim().is_empty() {
            self.refuse(
                "unnamed",
                &window.at,
                "A screen's window is matched by its title, and this one states none.".to_owned(),
                "Write the window's title as the application gives it. A window with no title is not read in this version.",
            );
        }
        let hole = match title_hole(&title) {
            Ok(hole) => hole,
            Err(why) => {
                self.refuse(
                    "title_invalid",
                    &window.at,
                    why.to_owned(),
                    "Write the title as the application gives it, with at most one `{name}` where the part that names the record is, and words beside it.",
                );
                None
            }
        };
        let window = WindowMatch {
            role: window.text("role")?.to_owned(),
            title,
            hole,
        };
        let ready = self.condition(doc.get("ready")?);
        let mut fields = BTreeMap::new();
        for entry in doc.get("fields").map(Located::entries).unwrap_or_default() {
            let control = entry.value.text("control").unwrap_or_default().to_owned();
            if let Some(locate) = self.locators(&entry.value) {
                fields.insert(entry.key.value.clone(), Field { control, locate });
            }
        }
        let rows = match doc.get("rows") {
            Some(rows) => self.rows(rows),
            None => None,
        };
        let mut actions = BTreeMap::new();
        for entry in doc.get("actions").map(Located::entries).unwrap_or_default() {
            let value = &entry.value;
            let locate = self.locators(value);
            let confirm = value.get("confirm").and_then(|cond| self.condition(cond));
            let key = value.text("key").map(str::to_owned);
            if key.is_some() && value.get("perform").is_some() {
                self.refuse(
                    "key_and_perform",
                    &value.at,
                    format!(
                        "The action `{}` states both `perform` and `key`; it is done one way only.",
                        shown(&entry.key.value)
                    ),
                    "Keep `key` for an action the element binds to a key, or `perform` for one it names.",
                );
                continue;
            }
            if let (Some(locate), Some(confirm)) = (locate, confirm) {
                actions.insert(
                    entry.key.value.clone(),
                    Action {
                        locate,
                        key,
                        perform: value.text("perform").unwrap_or(DEFAULT_PERFORM).to_owned(),
                        confirm,
                        commits: value.get("commits").and_then(Located::as_bool) == Some(true),
                    },
                );
            }
        }
        let mut variants = BTreeMap::new();
        for entry in doc
            .get("variants")
            .map(Located::entries)
            .unwrap_or_default()
        {
            if let Some(when) = entry
                .value
                .get("when")
                .and_then(|cond| self.condition(cond))
            {
                variants.insert(entry.key.value.clone(), Variant { when });
            }
        }
        if self.problems.len() != before {
            return None;
        }
        Some(Screen {
            surface: surface.to_owned(),
            window,
            ready: ready?,
            fields,
            rows,
            actions,
            variants,
        })
    }
}

/// The placeholder of a window's title, `{name}` with `name` a name in the
/// form target files use, and the words around it; `None` when it has
/// none. Braces around anything else are the title's own words. A title
/// with two placeholders, or with nothing beside its one, is refused: the
/// first is not built, and the second would match every window.
pub(crate) fn title_hole(title: &str) -> Result<Option<TitleHole>, &'static str> {
    let mut found = None;
    let mut from = 0;
    while let Some(open) = title[from..].find('{').map(|at| at + from) {
        let Some(close) = title[open..].find('}').map(|at| at + open) else {
            break;
        };
        let name = &title[open + 1..close];
        let named = name.starts_with(|c: char| c.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if named {
            if found.is_some() {
                return Err(
                    "A screen's window title holds more than one placeholder; one is built in this version.",
                );
            }
            found = Some(TitleHole {
                before: title[..open].to_owned(),
                name: name.to_owned(),
                after: title[close + 1..].to_owned(),
            });
        }
        from = close + 1;
    }
    if let Some(hole) = &found
        && hole.before.trim().is_empty()
        && hole.after.trim().is_empty()
    {
        return Err(
            "A screen's window title is one placeholder and nothing else, so it would match every window of the application.",
        );
    }
    Ok(found)
}

/// The `screens/` file kind, for `desktop` surfaces.
pub(crate) struct ScreenKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl ScreenKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: screen_schema(),
            kinds: vec![DESKTOP_SURFACE],
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

    /// A screen with an action that saves describes a write.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        declares_commit(doc)
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
                    "Report this to the desktop pack's author.",
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_name_is_a_code_of_this_pack() {
        for rule in [
            "role_unknown",
            "unnamed",
            "condition_invalid",
            "column_unknown",
            "not_encodable",
            "unknown_screen",
            "unknown_field",
            "writes_not_built",
            "action_missing",
            "unknown_action",
            "read_acts",
            "value_type",
            "password_set",
            "unreadable_write",
            "key_unshown",
            "title_invalid",
            "title_unbound",
            "title_write",
        ] {
            assert_eq!(code(rule).as_str(), format!("desktop.{rule}"));
        }
        for control in CONTROLS {
            assert!(a2m_core::ControlName::new(control).is_ok());
        }
    }

    #[test]
    fn a_title_holds_at_most_one_placeholder_and_words_beside_it() {
        let hole = |before: &str, name: &str, after: &str| TitleHole {
            before: before.to_owned(),
            name: name.to_owned(),
            after: after.to_owned(),
        };
        assert_eq!(title_hole("Example order"), Ok(None));
        assert_eq!(
            title_hole("Order {order_id}"),
            Ok(Some(hole("Order ", "order_id", "")))
        );
        assert_eq!(
            title_hole("Order {key} - Example"),
            Ok(Some(hole("Order ", "key", " - Example")))
        );
        // Braces round anything but a name are the title's own words.
        assert_eq!(title_hole("Notes {Draft}"), Ok(None));
        assert_eq!(title_hole("Notes {"), Ok(None));
        assert!(title_hole("{a} and {b}").is_err());
        assert!(title_hole(" {key} ").is_err());
        let window = WindowMatch {
            role: "frame".to_owned(),
            title: "Order {order_id}".to_owned(),
            hole: Some(hole("Order ", "order_id", "")),
        };
        assert_eq!(
            window.title_with(Some("1001")).as_deref(),
            Some("Order 1001")
        );
        assert_eq!(window.title_with(Some("")), None);
        assert_eq!(window.title_with(None), None);
        assert!(window.could_be("Order 1002"));
        assert!(!window.could_be("Order "));
        assert!(!window.could_be("Orders 1"));
    }
}
