//! The `screens/` file kind for surfaces of the kind `web`: its schema, its
//! compilation into the screens of the plan, and the lint rules over it.
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
    Action, Cond, Field, Fingerprint, Handling, LocateBy, Locator, PAYLOAD_FORMAT, ReadHow, Rows,
    Screen, ScreenSection, Segment, Variant,
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
];

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
        .optional("read", JsonSchema::words(&["value", "text", "checked"]));
    let action = JsonSchema::object()
        .required("locate", locate_schema())
        .optional("fingerprint", fingerprint_schema())
        .required("confirm", condition_schema(2))
        .optional("commits", JsonSchema::bool());
    let rows = JsonSchema::object()
        .required("locate", locate_schema())
        .optional("fingerprint", fingerprint_schema())
        .required("header", JsonSchema::non_empty_list(JsonSchema::text()))
        .required(
            "columns",
            JsonSchema::map_of(
                JsonSchema::object()
                    .required("header", JsonSchema::text())
                    .required("type", JsonSchema::field_type()),
            ),
        );
    let variant = JsonSchema::object()
        .required(
            "when",
            JsonSchema::object().required("visible", locator_schema(1)),
        )
        .required("handling", JsonSchema::words(&["absent", "stop"]));
    JsonSchema::object()
        .required("screen", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required(
            "reach",
            JsonSchema::object().required("url", JsonSchema::text()),
        )
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
fn is_name(text: &str) -> bool {
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
        if let Some(selector) = text("css") {
            kinds.push(LocateBy::Css(selector));
        }
        if kinds.len() != 1 || stray_contains {
            self.refuse(
                "locator_invalid",
                &doc.at,
                "A locator matches in exactly one way: `role` with `name`, `label`, `placeholder`, `text`, `test_id`, `title`, or `css`; `contains` goes with `text` only.".to_owned(),
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
            (Some("checked"), _) | (None, "checkbox") => ReadHow::Checked,
            _ => ReadHow::Value,
        };
        if (read == ReadHow::Checked) != (control == "checkbox") {
            self.refuse(
                "read_not_fitting",
                &doc.at,
                format!(
                    "The control `{}` cannot be read as the field says.",
                    shown(&control)
                ),
                "A `checkbox` is read as `checked`, and every other control as `value` or `text`.",
            );
            return None;
        }
        Some(Field {
            control,
            locate,
            fingerprint,
            read,
        })
    }

    fn action(&mut self, doc: &Located<Doc>) -> Option<Action> {
        let locate = self.locators(doc)?;
        let fingerprint = self.fingerprint(doc, &locate)?;
        Some(Action {
            locate,
            fingerprint,
            confirm: self.condition(doc.get("confirm")?)?,
            commits: doc.get("commits").and_then(Located::as_bool) == Some(true),
        })
    }

    fn rows(&mut self, doc: &Located<Doc>) -> Option<Rows> {
        let locate = self.locators(doc)?;
        let fingerprint = self.fingerprint(doc, &locate)?;
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
                _ => None,
            };
            if let (Some(when), Some(handling)) = (when, handling) {
                variants.insert(entry.key.value.clone(), Variant { when, handling });
            }
        }
        if self.problems.len() != before {
            return None;
        }
        Some(Screen {
            surface: surface.to_owned(),
            reach: reach?,
            ready: ready?,
            fields,
            actions,
            rows,
            variants,
        })
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
