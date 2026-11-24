//! The widget drivers of the controls a page draws for itself: a filtering
//! dropdown, a slider, a file input, a grid, a tree, tabs, and an accordion.
//! Each is built from the base's primitives (looking for an element by role
//! and name, a click, a key, a read of one element) and from what the page
//! itself says of the control's state: `aria-selected`, `aria-expanded`,
//! `aria-valuenow`. A control that keeps its state anywhere else, or that
//! does not do what the rule expects of it, is a stop with a named class
//! (rule 7), never a guess.
//!
//! Every rule sets in one step, and sets nothing it does not then find
//! shown: a rule that sent input and does not find the control as it meant
//! it stops with the fate of the input unknown.

use a2m_core::{Decimal, Value};
use a2m_pack_web_browser::{
    BrowserError, Deadline, Element, Input, Key, Primitive, Query, RawValue, ReadWhat, Reason,
};
use a2m_provider::StepFailure;

use super::{Rule, Session, browser_failure, codes, fail, widget_text};
use crate::codes::shown;
use crate::model::Field;
use a2m_pack_web_browser::By;

/// How long the part of a control that is drawn after the control is used
/// is waited for.
const PART_APPEARS_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// The most keys a slider is given to reach its value.
const MOST_KEYS: usize = 500;

/// The field a widget rule works on, with the element its locators found.
pub(super) struct Chosen<'a> {
    pub field: &'a Field,
    pub element: &'a Element,
    /// Which locator of the field found the element.
    pub locator: usize,
    /// The key of the row of a grid, as the binding gave it.
    pub row: Option<&'a Value>,
    /// The field and its screen, in words, for a message.
    pub what: &'a str,
}

/// A stop after input reached the page: the control does not do what its
/// rule expects, and what became of the input is not known.
fn stop_after_input(message: String, next: &str) -> StepFailure {
    StepFailure::unknown(fail(&codes::row::UNEXPECTED_CONTROL, message, next))
}

/// A stop of a read: the page answered with something its rule has no
/// meaning for.
fn stop_reading(message: String, next: &str) -> StepFailure {
    StepFailure::answered(fail(&codes::row::UNEXPECTED_CONTROL, message, next))
}

/// What a failure after input is: the input is not taken back, so its fate
/// is not known, whatever the failure says of the step that failed.
fn sent_already(failure: StepFailure) -> StepFailure {
    StepFailure::unknown(failure.failure)
}

/// The role and the attribute that say which choice of `tabs` or an
/// `accordion` is on.
fn choice_parts(rule: Rule) -> (&'static str, &'static str) {
    if rule == Rule::Tabs {
        ("tab", "aria-selected")
    } else {
        ("button", "aria-expanded")
    }
}

impl Session {
    /// The query that finds the group (or the grid, or the tree) the
    /// field's locators found, to look for its parts inside.
    fn whole(chosen: &Chosen<'_>) -> Query {
        let frame = chosen
            .field
            .fingerprint
            .as_ref()
            .and_then(|print| print.frame.as_ref());
        chosen.field.locate[chosen.locator].query_in(frame)
    }

    /// The one element a query finds, once the page shows one: a part of a
    /// control that is drawn after the control is used, such as the options
    /// of a dropdown that loads them, or the children of a tree item that
    /// was opened. The wait is for a moment, not for the step's whole time,
    /// and a part that does not appear is one the locators do not find.
    async fn appears(
        &self,
        what: &str,
        query: &Query,
        deadline: Deadline,
    ) -> Result<Element, StepFailure> {
        let soon = Deadline::after(deadline.remaining().min(PART_APPEARS_WITHIN));
        match self
            .tab()
            .wait(&Primitive::Visible(query.clone()), soon)
            .await
        {
            Ok(()) => {}
            Err(BrowserError::Timeout { .. }) if !deadline.passed() => {
                return Err(browser_failure(&BrowserError::NotFound, what));
            }
            Err(error) => return Err(browser_failure(&error, what)),
        }
        self.tab()
            .locate(query, deadline)
            .await
            .map_err(|error| browser_failure(&error, what))
    }

    /// The text of an element, for one thing read of it.
    async fn read_text(
        &self,
        element: &Element,
        what: &ReadWhat,
        words: &str,
        deadline: Deadline,
    ) -> Result<String, StepFailure> {
        match self.tab().read(element, what, deadline).await {
            Ok(RawValue::Text(text)) => Ok(text),
            Ok(_) => Err(browser_failure(
                &BrowserError::NotActionable(Reason::WrongControl),
                words,
            )),
            Err(error) => Err(browser_failure(&error, words)),
        }
    }

    /// The text of an attribute, when the element has it.
    async fn attribute(
        &self,
        element: &Element,
        name: &str,
        words: &str,
        deadline: Deadline,
    ) -> Result<Option<String>, StepFailure> {
        match self
            .tab()
            .read(
                element,
                &ReadWhat::Attribute {
                    name: name.to_owned(),
                },
                deadline,
            )
            .await
        {
            Ok(RawValue::Text(text)) => Ok(Some(text)),
            Ok(_) => Ok(None),
            Err(error) => Err(browser_failure(&error, words)),
        }
    }

    /// The key of the row a grid cell is in: the text its first cell shows.
    fn row_key(chosen: &Chosen<'_>) -> Result<String, StepFailure> {
        widget_text(chosen.row)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| codes::not_usable("a grid cell is set or read with no key of its row"))
    }

    /// The place of a grid cell among the cells of its row.
    fn column(chosen: &Chosen<'_>) -> Result<usize, StepFailure> {
        chosen
            .field
            .column
            .and_then(|column| usize::try_from(column).ok())
            .ok_or_else(|| codes::not_usable("a grid field names no column"))
    }

    /// Sets a control with a rule of its own. Returns the name of the
    /// strategy that held.
    pub(super) async fn set_widget(
        &self,
        rule: Rule,
        chosen: &Chosen<'_>,
        text: &str,
        wrong_value: &(dyn Fn() -> StepFailure + Sync),
        deadline: Deadline,
    ) -> Result<&'static str, StepFailure> {
        match rule {
            Rule::Combo => {
                let label = if chosen.field.options.is_empty() {
                    text.to_owned()
                } else {
                    chosen
                        .field
                        .options
                        .get(text)
                        .ok_or_else(wrong_value)?
                        .clone()
                };
                self.choose_option(chosen, &label, deadline).await?;
                Ok("choose")
            }
            Rule::Slider => {
                let target = Decimal::parse(text).map_err(|_| wrong_value())?;
                self.slide(chosen, target, deadline).await?;
                Ok("keys")
            }
            Rule::File => {
                self.attach(chosen, text, deadline).await?;
                Ok("attach")
            }
            Rule::Grid => {
                self.edit_cell(chosen, text, deadline).await?;
                Ok("edit")
            }
            Rule::Tree => {
                let path: Vec<&str> = text.split('/').collect();
                if path.iter().any(|label| label.is_empty()) {
                    return Err(wrong_value());
                }
                self.select_path(chosen, text, &path, deadline).await?;
                Ok("keys")
            }
            Rule::Tabs | Rule::Accordion => {
                let label = chosen.field.options.get(text).ok_or_else(wrong_value)?;
                self.activate(rule, chosen, label, deadline).await?;
                Ok("click")
            }
            Rule::Text | Rule::Select | Rule::Check | Rule::Radio => Err(codes::not_usable(
                "a control of the base rules was given to a widget rule",
            )),
        }
    }

    /// Reads a control with a rule of its own, as a value.
    pub(super) async fn read_widget(
        &self,
        rule: Rule,
        chosen: &Chosen<'_>,
        deadline: Deadline,
    ) -> Result<Value, StepFailure> {
        let what = chosen.what;
        match rule {
            Rule::Combo => {
                let shown_now = self
                    .read_text(chosen.element, &ReadWhat::Value, what, deadline)
                    .await?;
                if chosen.field.options.is_empty() {
                    return Ok(Value::Text(shown_now));
                }
                let code = chosen
                    .field
                    .options
                    .iter()
                    .find(|(_, label)| **label == shown_now)
                    .map(|(code, _)| code.clone());
                code.map(Value::Text).ok_or_else(|| {
                    stop_reading(
                        format!(
                            "{what} shows `{}`, which none of the field's `options` is the label of.",
                            shown(&shown_now)
                        ),
                        "Look at the page; the control shows a choice the field's `options` do not list.",
                    )
                })
            }
            Rule::Slider => Ok(Value::Text(
                self.slider_value(chosen.element, what, deadline)
                    .await?
                    .to_string(),
            )),
            Rule::File => Ok(Value::Text(
                self.read_text(chosen.element, &ReadWhat::Files, what, deadline)
                    .await?,
            )),
            Rule::Grid => {
                let key = Self::row_key(chosen)?;
                let column = Self::column(chosen)?;
                let row_words = format!("the row `{}` of {what}", shown(&key));
                let row = self
                    .tab()
                    .locate(
                        &Query::new(By::RowKeyed { key }).within(Self::whole(chosen)),
                        deadline,
                    )
                    .await
                    .map_err(|error| browser_failure(&error, &row_words))?;
                Ok(Value::Text(
                    self.read_text(
                        &row,
                        &ReadWhat::Cell { index: column },
                        &row_words,
                        deadline,
                    )
                    .await?,
                ))
            }
            Rule::Tree => match self
                .tab()
                .read(chosen.element, &ReadWhat::TreePath, deadline)
                .await
            {
                Ok(RawValue::Text(path)) => Ok(Value::Text(path)),
                Ok(_) => Err(stop_reading(
                    format!("No item of {what} is selected."),
                    "Look at the page; the tree shows no selected item, or marks it some other way than `aria-selected`.",
                )),
                Err(error) => Err(browser_failure(&error, what)),
            },
            Rule::Tabs | Rule::Accordion => {
                let (role, attribute) = choice_parts(rule);
                for (code, label) in &chosen.field.options {
                    let item = self
                        .tab()
                        .locate(
                            &Query::role(role, label).within(Self::whole(chosen)),
                            deadline,
                        )
                        .await
                        .map_err(|error| browser_failure(&error, what))?;
                    if self
                        .attribute(&item, attribute, what, deadline)
                        .await?
                        .as_deref()
                        == Some("true")
                    {
                        return Ok(Value::Text(code.clone()));
                    }
                }
                Err(stop_reading(
                    format!("No choice of {what} is on."),
                    "Look at the page; the control shows no choice, or one the field's `options` do not list.",
                ))
            }
            Rule::Text | Rule::Select | Rule::Check | Rule::Radio => Err(codes::not_usable(
                "a control of the base rules was given to a widget rule",
            )),
        }
    }

    /// A dropdown that filters as it is typed in: the text is typed, the
    /// option of exactly that label is waited for, since a page may load
    /// its options late, and pressed. The control must then show the label.
    async fn choose_option(
        &self,
        chosen: &Chosen<'_>,
        label: &str,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let what = chosen.what;
        self.tab()
            .input(
                chosen.element,
                &Input::Fill(label.to_owned()),
                deadline,
                what,
            )
            .await?;
        let option = self
            .appears(what, &Query::role("option", label), deadline)
            .await
            .map_err(sent_already)?;
        self.tab()
            .input(&option, &Input::Click, deadline, what)
            .await
            .map_err(sent_already)?;
        let held = self
            .read_text(chosen.element, &ReadWhat::Value, what, deadline)
            .await
            .map_err(sent_already)?;
        if held == label {
            Ok(())
        } else {
            Err(stop_after_input(
                format!(
                    "{what} does not show the option it was given after the option was chosen."
                ),
                "Look at the page; the control keeps its choice somewhere other than its own value.",
            ))
        }
    }

    /// The number a slider shows: its own value, or, for a slider the page
    /// draws, the `aria-valuenow` it keeps.
    async fn slider_value(
        &self,
        element: &Element,
        what: &str,
        deadline: Deadline,
    ) -> Result<Decimal, StepFailure> {
        let text = match self.tab().read(element, &ReadWhat::Value, deadline).await {
            Ok(RawValue::Text(text)) => text,
            Ok(RawValue::Absent) => self
                .attribute(element, "aria-valuenow", what, deadline)
                .await?
                .ok_or_else(|| {
                    browser_failure(&BrowserError::NotActionable(Reason::WrongControl), what)
                })?,
            Ok(RawValue::Flag(_)) => {
                return Err(browser_failure(
                    &BrowserError::NotActionable(Reason::WrongControl),
                    what,
                ));
            }
            Err(error) => return Err(browser_failure(&error, what)),
        };
        Decimal::parse(text.trim()).map_err(|_| {
            stop_reading(
                format!("{what} shows `{}`, which is not a number.", shown(&text)),
                "Look at the page; a slider shows a number.",
            )
        })
    }

    /// Moves a slider to a value by its keys, as a person without a mouse
    /// would: toward the value, one key at a time, reading the slider after
    /// each. A step that does not move it, one that passes the value, and
    /// too many keys all stop.
    async fn slide(
        &self,
        chosen: &Chosen<'_>,
        target: Decimal,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let what = chosen.what;
        let mut now = self
            .slider_value(chosen.element, what, deadline)
            .await
            .map_err(|failure| StepFailure::not_sent(failure.failure))?;
        let mut sent = false;
        for _ in 0..MOST_KEYS {
            if now == target {
                return Ok(());
            }
            let upward = now < target;
            let key = if upward {
                Key::ArrowRight
            } else {
                Key::ArrowLeft
            };
            let pressed = self
                .tab()
                .input(chosen.element, &Input::Press(key), deadline, what)
                .await;
            pressed.map_err(|failure| if sent { sent_already(failure) } else { failure })?;
            sent = true;
            let next = self
                .slider_value(chosen.element, what, deadline)
                .await
                .map_err(sent_already)?;
            if next == now || (next < target) != upward && next != target {
                return Err(stop_after_input(
                    format!(
                        "{what} did not move toward `{target}` by its keys: it went from `{now}` to `{next}`."
                    ),
                    "Look at the page; the slider takes other keys, or its steps do not land on the value.",
                ));
            }
            now = next;
        }
        if now == target {
            return Ok(());
        }
        Err(stop_after_input(
            format!("{what} did not reach `{target}` in {MOST_KEYS} keys."),
            "Look at the page; the slider's steps are too small to cross by its keys.",
        ))
    }

    /// Gives a file input a file of the staging folder, by its name. A name
    /// that is not one plain name, or that no file of the folder has, is
    /// refused before anything is sent: no path from a caller is opened.
    async fn attach(
        &self,
        chosen: &Chosen<'_>,
        name: &str,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let what = chosen.what;
        let not_staged = |why: &str| {
            StepFailure::not_sent(fail(
                &codes::row::FILE_NOT_STAGED,
                format!(
                    "The file `{}` for {what} cannot be given: {why}.",
                    shown(name)
                ),
                "Put the file in the staging folder under the name, and give that name; no path is taken.",
            ))
        };
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
            return Err(not_staged("it is not one plain name of a file"));
        }
        let folder = self
            .staging
            .as_ref()
            .map_err(|failure| StepFailure::not_sent(failure.clone()))?;
        let path = folder.join(name).map_err(StepFailure::not_sent)?;
        if !path.exists() || path.is_dir() {
            return Err(not_staged("the staging folder holds no file of that name"));
        }
        self.tab()
            .input(
                chosen.element,
                &Input::Attach(path.as_path().to_path_buf()),
                deadline,
                what,
            )
            .await?;
        let held = self
            .read_text(chosen.element, &ReadWhat::Files, what, deadline)
            .await
            .map_err(sent_already)?;
        if held == name {
            Ok(())
        } else {
            Err(stop_after_input(
                format!("{what} does not show the file it was given."),
                "Look at the page; the file input takes no file, or changes its name.",
            ))
        }
    }

    /// Edits one cell of a grid, in the row of a key: the cell is pressed,
    /// the editor it opens takes the text, and Enter ends the edit. A cell
    /// that opens no editor is a stop. The cell must then show the text.
    async fn edit_cell(
        &self,
        chosen: &Chosen<'_>,
        text: &str,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let key = Self::row_key(chosen)?;
        let column = Self::column(chosen)?;
        let what = chosen.what;
        let cell_words = format!("the cell of the row `{}` of {what}", shown(&key));
        let cell_query = Query::new(By::CellKeyed { key, column }).within(Self::whole(chosen));
        let cell = self
            .tab()
            .locate(&cell_query, deadline)
            .await
            .map_err(|error| browser_failure(&error, &cell_words))?;
        self.tab()
            .input(&cell, &Input::Click, deadline, &cell_words)
            .await?;
        let editor_query = Query::new(By::Css {
            selector: "input".to_owned(),
        })
        .within(cell_query.clone());
        let editor = self
            .appears(&cell_words, &editor_query, deadline)
            .await
            .map_err(sent_already)?;
        self.tab()
            .input(
                &editor,
                &Input::Fill(text.to_owned()),
                deadline,
                &cell_words,
            )
            .await
            .map_err(sent_already)?;
        self.tab()
            .input(&editor, &Input::Press(Key::Enter), deadline, &cell_words)
            .await
            .map_err(sent_already)?;
        self.tab()
            .wait(&Primitive::Hidden(editor_query), deadline)
            .await
            .map_err(|error| sent_already(browser_failure(&error, &cell_words)))?;
        let cell = self
            .tab()
            .locate(&cell_query, deadline)
            .await
            .map_err(|error| sent_already(browser_failure(&error, &cell_words)))?;
        let held = self
            .read_text(&cell, &ReadWhat::Text, &cell_words, deadline)
            .await
            .map_err(sent_already)?;
        if held.trim() == text {
            Ok(())
        } else {
            Err(stop_after_input(
                format!("{cell_words} does not show the text it was given after the edit ended."),
                "Look at the page; the grid ends an edit another way, or refuses the text.",
            ))
        }
    }

    /// Opens a tree along a path of labels, and selects the item it ends
    /// at: each item but the last is opened by the key that opens it if it
    /// is closed, and the last is selected by Enter. An item that cannot be
    /// opened, and a path that is not found, stop.
    async fn select_path(
        &self,
        chosen: &Chosen<'_>,
        text: &str,
        path: &[&str],
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let what = chosen.what;
        let mut scope = Self::whole(chosen);
        let mut sent = false;
        for (place, label) in path.iter().enumerate() {
            let item_words = format!("the item `{}` of {what}", shown(label));
            let query = Query::role("treeitem", label).within(scope);
            let mut item = self.appears(&item_words, &query, deadline).await;
            if sent {
                item = item.map_err(sent_already);
            }
            let item = item?;
            let last = place + 1 == path.len();
            let key = if last {
                Some(Key::Enter)
            } else {
                match self
                    .attribute(&item, "aria-expanded", &item_words, deadline)
                    .await
                    .map_err(|failure| if sent { sent_already(failure) } else { failure })?
                    .as_deref()
                {
                    Some("false") => Some(Key::ArrowRight),
                    Some("true") => None,
                    _ => {
                        return Err(if sent {
                            stop_after_input(
                                format!(
                                    "{item_words} cannot be opened: it does not say whether it is open."
                                ),
                                "Look at the page; an item that holds others says `aria-expanded`.",
                            )
                        } else {
                            StepFailure::not_sent(fail(
                                &codes::row::UNEXPECTED_CONTROL,
                                format!(
                                    "{item_words} cannot be opened: it does not say whether it is open."
                                ),
                                "Look at the page; an item that holds others says `aria-expanded`.",
                            ))
                        });
                    }
                }
            };
            if let Some(key) = key {
                let pressed = self
                    .tab()
                    .input(&item, &Input::Press(key), deadline, &item_words)
                    .await;
                pressed.map_err(|failure| if sent { sent_already(failure) } else { failure })?;
                sent = true;
            }
            scope = query;
        }
        let held = self
            .read_text(chosen.element, &ReadWhat::TreePath, what, deadline)
            .await
            .map_err(sent_already)?;
        if held == text {
            Ok(())
        } else {
            Err(stop_after_input(
                format!(
                    "{what} selects `{}` and not the path it was given.",
                    shown(&held)
                ),
                "Look at the page; the tree selects another way, or the path ends at an item that opens instead of selecting.",
            ))
        }
    }

    /// Turns a tab on, or opens an accordion's section, by pressing it
    /// unless it already is. The control must then say it is.
    async fn activate(
        &self,
        rule: Rule,
        chosen: &Chosen<'_>,
        label: &str,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let (role, attribute) = choice_parts(rule);
        let what = chosen.what;
        let words = format!("the choice `{}` of {what}", shown(label));
        let query = Query::role(role, label).within(Self::whole(chosen));
        let item = self.appears(&words, &query, deadline).await?;
        let on = |text: Option<String>| text.as_deref() == Some("true");
        if on(self.attribute(&item, attribute, &words, deadline).await?) {
            return Ok(());
        }
        self.tab()
            .input(&item, &Input::Click, deadline, &words)
            .await?;
        let item = self
            .tab()
            .locate(&query, deadline)
            .await
            .map_err(|error| sent_already(browser_failure(&error, &words)))?;
        if on(self
            .attribute(&item, attribute, &words, deadline)
            .await
            .map_err(sent_already)?)
        {
            Ok(())
        } else {
            Err(stop_after_input(
                format!("{words} does not say it is on after it was pressed."),
                "Look at the page; the control keeps its state another way than its attribute.",
            ))
        }
    }
}
