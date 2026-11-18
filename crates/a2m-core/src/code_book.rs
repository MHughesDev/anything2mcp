//! The table form in which every crate declares its failure codes, and the
//! book that reads the tables of many crates together.
//!
//! A crate declares its codes once, with [`failure_codes!`](crate::failure_codes):
//! each code with its class, what it means, what to do about it, and whether
//! the same call may end differently if it is made again unchanged. The macro
//! gives the crate its constants, an `ALL` list of classes and codes, and a
//! `TABLE` of [`CodeEntry`]. `tools/failure_codes.py` reads the same rows as
//! text and writes the technical reference's table from them.

use std::collections::BTreeMap;

use crate::failure::{Failure, FailureClass, FailureCode};
use crate::name::{is_dotted_name, is_name};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// Whether a failure may end differently when the same call is made
    /// again, unchanged, with nobody acting in between.
    pub enum Passes: "passes" {
        /// It may: a queue drains, a lock is let go, a connection comes back.
        ByItself => "by_itself",
        /// It does not: the call, a setting, a file, or a person must change
        /// first.
        NotByItself => "not_by_itself",
    }
}

impl Passes {
    /// What a code of `class` says when its table states nothing:
    /// [`Passes::ByItself`] for `transient`, [`Passes::NotByItself`] for
    /// every other class. A `busy` code must state it; the macro refuses one
    /// that does not.
    #[must_use]
    pub const fn default_for(class: FailureClass) -> Self {
        match class {
            FailureClass::Transient => Self::ByItself,
            _ => Self::NotByItself,
        }
    }
}

/// The action a table names as the usual way out of a code, and what it is
/// for, such as `test.run`, "to run the gates". It carries no arguments: the
/// dispatcher fills them from the call, or gives no next action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ThenAction {
    /// The action's name, two names joined by one dot.
    pub action: &'static str,
    /// What it is for, as a phrase that follows the action's name.
    pub when: &'static str,
}

/// One row of a crate's table of failure codes, as
/// [`failure_codes!`](crate::failure_codes) writes it.
///
/// Every field is set when the crate is compiled. A row is made only by the
/// macro, which refuses an empty meaning or next step, a malformed code or
/// action, and a `busy` code that does not say whether it passes by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CodeEntry {
    /// The package that declares the code, such as `a2m-expr`.
    pub declared_in: &'static str,
    /// The code's one class.
    pub class: FailureClass,
    /// The code, such as `expression_invalid`.
    pub code: &'static str,
    /// What the code means, in one sentence.
    pub meaning: &'static str,
    /// What to do about it, as an instruction. A failure's own next step,
    /// written where it is made, says it for the case at hand; this is the
    /// general one.
    pub next: &'static str,
    /// The action that is the usual way out, when there is one.
    pub then: Option<ThenAction>,
    /// Whether the same call may end differently made again unchanged.
    pub passes: Passes,
}

impl CodeEntry {
    /// The row the macro writes. Not part of the interface: a row is made
    /// only by [`failure_codes!`](crate::failure_codes), which checks it.
    #[doc(hidden)]
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "called only by the macro, one argument per field of the row"
    )]
    pub const fn __row(
        declared_in: &'static str,
        class: FailureClass,
        code: &'static str,
        meaning: &'static str,
        next: &'static str,
        passes: Option<Passes>,
        then: Option<(&'static str, &'static str)>,
    ) -> Self {
        assert!(
            !blank(meaning),
            "a failure code's table row needs a meaning: `meaning \"...\"`"
        );
        assert!(
            !blank(next),
            "a failure code's table row needs a next step, written as an instruction: `next \"...\"`"
        );
        assert!(
            !(matches!(class, FailureClass::Busy) && passes.is_none()),
            "a `busy` code says whether it passes by itself: `passes by_itself` or `passes not_by_itself`"
        );
        let then = match then {
            Some((action, when)) => {
                assert!(
                    is_dotted_name(action, 64),
                    "a table's `then` names an action: two names joined by one dot"
                );
                assert!(!blank(when), "a table's `then` says what the action is for");
                Some(ThenAction { action, when })
            }
            None => None,
        };
        assert!(
            is_name(code, FailureCode::LONGEST),
            "a failure code is lowercase letters, digits, and underscores, starting with a letter"
        );
        Self {
            declared_in,
            class,
            code,
            meaning,
            next,
            then,
            passes: match passes {
                Some(passes) => passes,
                None => Passes::default_for(class),
            },
        }
    }

    /// The code as a [`FailureCode`].
    #[must_use]
    pub const fn failure_code(&self) -> FailureCode {
        FailureCode::fixed(self.code)
    }

    /// A failure of this code, in the row's class, with `message` and
    /// `next` for the case at hand. Give the row's own [`next`](Self::next)
    /// where the case adds nothing to it. The class comes from the row, so
    /// the place that makes the failure cannot give the code another one.
    #[must_use]
    pub fn failure(&self, message: impl Into<String>, next: impl Into<String>) -> Failure {
        Failure::new(self.class, self.failure_code(), message, next)
    }
}

/// Whether `text` holds nothing but white space.
const fn blank(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_whitespace() {
            return false;
        }
        i += 1;
    }
    true
}

/// The tables of many crates read together: what each code means and
/// whether it passes by itself.
///
/// One code may be declared by more than one crate, with one class. A code
/// declared with two classes is refused when the book is made.
#[derive(Debug, Clone, Default)]
pub struct CodeBook {
    by_code: BTreeMap<&'static str, &'static CodeEntry>,
}

impl CodeBook {
    /// A book of `tables`, each a crate's `TABLE`. Where two tables declare
    /// one code, the row of the first is the one [`CodeBook::explain`] gives.
    ///
    /// # Errors
    ///
    /// A failure of class `input`, code `schema_invalid`, when one code is
    /// declared with two classes; it names both crates.
    pub fn new(tables: &[&'static [CodeEntry]]) -> Result<Self, Failure> {
        let mut by_code: BTreeMap<&'static str, &'static CodeEntry> = BTreeMap::new();
        for table in tables {
            for entry in *table {
                match by_code.get(entry.code) {
                    Some(first) if first.class != entry.class => {
                        return Err(Failure::new(
                            FailureClass::Input,
                            FailureCode::SCHEMA_INVALID,
                            format!(
                                "The failure code `{}` is declared as `{}` in {} and as `{}` in {}.",
                                entry.code,
                                first.class,
                                first.declared_in,
                                entry.class,
                                entry.declared_in
                            ),
                            "Give the code one class: change the row in one of the two crates, or rename the code there.",
                        ));
                    }
                    Some(_) => {}
                    None => {
                        by_code.insert(entry.code, entry);
                    }
                }
            }
        }
        Ok(Self { by_code })
    }

    /// The row for `code`, when a table in the book declares it.
    #[must_use]
    pub fn explain(&self, code: &str) -> Option<&'static CodeEntry> {
        self.by_code.get(code).copied()
    }

    /// Whether a failure of `code` and `class` passes by itself: the row's
    /// answer when the book holds the code with that class, else the class's
    /// default ([`Passes::default_for`]).
    #[must_use]
    pub fn passes(&self, code: &str, class: FailureClass) -> Passes {
        match self.by_code.get(code) {
            Some(entry) if entry.class == class => entry.passes,
            _ => Passes::default_for(class),
        }
    }

    /// Every row, one per code, in the order of the codes.
    pub fn entries(&self) -> impl Iterator<Item = &'static CodeEntry> + '_ {
        self.by_code.values().copied()
    }
}

/// Declares a crate's failure codes, each once, with its class, what it
/// means, what to do about it, and, where it applies, whether it passes by
/// itself and the action that is the usual way out.
///
/// ```
/// mod codes {
///     a2m_core::failure_codes! {
///         MANIFEST_INVALID = Input, "manifest_invalid",
///             meaning "A manifest does not parse, or a field of it is wrong.",
///             next "Correct the field the message names, then load the manifest again.";
///         QUEUE_FULL = Busy, "queue_full",
///             meaning "A queue is full.",
///             next "Call again after the wait the failure gives.",
///             passes by_itself;
///         GATES_NOT_PASSED = Input, "gates_not_passed",
///             meaning "A version's gates have not passed.",
///             next "Run the gates, then publish again.",
///             then ("test.run", "to run the gates");
///     }
/// }
/// assert_eq!(codes::QUEUE_FULL.as_str(), "queue_full");
/// assert_eq!(codes::ALL.len(), 3);
/// assert_eq!(codes::TABLE[1].passes, a2m_core::Passes::ByItself);
/// assert_eq!(codes::TABLE[2].then.map(|then| then.action), Some("test.run"));
/// ```
///
/// A row with an empty next step does not compile:
///
/// ```compile_fail
/// mod codes {
///     a2m_core::failure_codes! {
///         MANIFEST_INVALID = Input, "manifest_invalid",
///             meaning "A manifest does not parse, or a field of it is wrong.",
///             next " ";
///     }
/// }
/// assert_eq!(codes::ALL.len(), 1);
/// ```
///
/// Nor does a `busy` row that does not say whether it passes by itself:
///
/// ```compile_fail
/// mod codes {
///     a2m_core::failure_codes! {
///         QUEUE_FULL = Busy, "queue_full",
///             meaning "A queue is full.",
///             next "Call again after the wait the failure gives.";
///     }
/// }
/// assert_eq!(codes::ALL.len(), 1);
/// ```
///
/// Each row is `NAME = Class, "code", meaning "...", next "..."`, then
/// optionally `, passes by_itself` or `, passes not_by_itself`, then
/// optionally `, then ("noun.verb", "what it is for")`, and a `;`. A `busy`
/// row must say `passes`; every other class takes its default when it does
/// not ([`Passes::default_for`]). An empty meaning or next step, a malformed
/// code, and a `then` that names no action fail when the crate is compiled.
///
/// It writes, where it is called: a module `row` with a [`CodeEntry`]
/// constant per row, from which a failure of the code is made
/// ([`CodeEntry::failure`]); a [`FailureCode`] constant of the same name per
/// row, with its meaning and next step as its documentation; `ALL`, every
/// class and code; and `TABLE`, every row. Call it once per crate, in a
/// public module named `codes`, so that a book can list `<crate>::codes::TABLE`.
#[macro_export]
macro_rules! failure_codes {
    (@passes) => { ::core::option::Option::None };
    (@passes by_itself) => { ::core::option::Option::Some($crate::Passes::ByItself) };
    (@passes not_by_itself) => { ::core::option::Option::Some($crate::Passes::NotByItself) };
    (@then) => { ::core::option::Option::None };
    (@then $action:literal, $when:literal) => {
        ::core::option::Option::Some(($action, $when))
    };
    (
        $(
            $name:ident = $class:ident, $code:literal,
                meaning $meaning:literal,
                next $next:literal
                $(, passes $passes:ident)?
                $(, then ($action:literal, $when:literal))?
            ;
        )+
    ) => {
        /// The row of each code, by the code's constant name. A failure of
        /// one of these codes is made from its row, with `CodeEntry::failure`,
        /// so that its class is the table's and no call site states one.
        pub mod row {
            $(
                #[doc = $meaning]
                pub const $name: $crate::CodeEntry = $crate::CodeEntry::__row(
                    env!("CARGO_PKG_NAME"),
                    $crate::FailureClass::$class,
                    $code,
                    $meaning,
                    $next,
                    $crate::failure_codes!(@passes $($passes)?),
                    $crate::failure_codes!(@then $($action, $when)?),
                );
            )+
        }

        $(
            #[doc = $meaning]
            #[doc = ""]
            #[doc = concat!("What to do: ", $next)]
            pub const $name: $crate::FailureCode = row::$name.failure_code();
        )+

        /// Every code of this crate with its one class.
        pub const ALL: &[($crate::FailureClass, &str)] =
            &[ $( ($crate::FailureClass::$class, $code) ),+ ];

        /// Every code of this crate, with its class, meaning, next step,
        /// whether it passes by itself, and the action that is the usual
        /// way out.
        pub const TABLE: &[$crate::CodeEntry] = &[ $( row::$name ),+ ];
    };
}

#[cfg(test)]
mod tests {
    use super::{CodeBook, CodeEntry, Passes};
    use crate::FailureClass;

    mod first {
        crate::failure_codes! {
            QUEUE_FULL = Busy, "queue_full",
                meaning "A queue is full.",
                next "Call again after the wait the failure gives.",
                passes by_itself;
            ON_HOLD = Busy, "on_hold",
                meaning "The run is held.",
                next "Ask whoever held the run to let it go.",
                passes not_by_itself;
            GATES_NOT_PASSED = Input, "gates_not_passed",
                meaning "A version's gates have not passed.",
                next "Run the gates, then publish again.",
                then ("test.run", "to run the gates");
            TIMED_OUT = Transient, "timed_out",
                meaning "The system did not answer in time.",
                next "Call again.";
        }
    }

    mod second {
        crate::failure_codes! {
            QUEUE_FULL = Input, "queue_full",
                meaning "Planted: the same code with another class.",
                next "Rename it.";
            TIMED_OUT = Transient, "timed_out",
                meaning "Another crate's sentence for the same code.",
                next "Call again later.";
        }
    }

    #[test]
    fn a_table_entry_carries_class_meaning_next_and_passes() {
        assert_eq!(first::ALL.len(), first::TABLE.len());
        let full = &first::TABLE[0];
        assert_eq!(full.declared_in, "a2m-core");
        assert_eq!(full.class, FailureClass::Busy);
        assert_eq!(full.code, "queue_full");
        assert_eq!(full.meaning, "A queue is full.");
        assert_eq!(full.next, "Call again after the wait the failure gives.");
        assert_eq!(full.passes, Passes::ByItself);
        assert_eq!(full.then, None);
        assert_eq!(first::QUEUE_FULL, full.failure_code());
        assert_eq!(&first::row::QUEUE_FULL, full);
        // A failure made from a row has the row's class; nothing else gives it one.
        let made = first::row::GATES_NOT_PASSED
            .failure("Gates failed.", first::row::GATES_NOT_PASSED.next);
        assert_eq!(made.class(), FailureClass::Input);
        assert_eq!(made.code(), &first::GATES_NOT_PASSED);
        assert_eq!(made.next_step(), "Run the gates, then publish again.");
        assert_eq!(first::TABLE[1].passes, Passes::NotByItself);
        let gates = &first::TABLE[2];
        let then = gates.then.unwrap();
        assert_eq!((then.action, then.when), ("test.run", "to run the gates"));
        // Unstated, a class takes its default: only `transient` passes.
        assert_eq!(gates.passes, Passes::NotByItself);
        assert_eq!(first::TABLE[3].passes, Passes::ByItself);
        assert_eq!(first::ALL[3], (FailureClass::Transient, "timed_out"));
        assert_eq!(second::ALL[0], (FailureClass::Input, "queue_full"));
        let names = [
            first::ON_HOLD,
            first::GATES_NOT_PASSED,
            first::TIMED_OUT,
            second::QUEUE_FULL,
            second::TIMED_OUT,
        ];
        assert_eq!(
            names.map(|code| code.as_str().len()),
            [7, 16, 9, 10, 9],
            "each constant is its code"
        );
    }

    #[test]
    fn a_book_refuses_one_code_with_two_classes() {
        let refused = CodeBook::new(&[first::TABLE, second::TABLE]).unwrap_err();
        assert_eq!(refused.class(), FailureClass::Input);
        assert_eq!(refused.code().as_str(), "schema_invalid");
        assert!(
            refused.message().contains("`queue_full`")
                && refused.message().contains("`busy`")
                && refused.message().contains("`input`"),
            "{refused}"
        );
        // One code in two crates with one class is one row: the first's.
        let book = CodeBook::new(&[first::TABLE, &second::TABLE[1..]]).unwrap();
        let timed_out: &CodeEntry = book.explain("timed_out").unwrap();
        assert_eq!(timed_out.meaning, "The system did not answer in time.");
        assert_eq!(book.entries().count(), 4);
        assert!(book.explain("no_such_code").is_none());
    }

    #[test]
    fn a_book_says_whether_a_code_passes_by_itself_or_falls_back_to_its_class() {
        let book = CodeBook::new(&[first::TABLE]).unwrap();
        assert_eq!(
            book.passes("queue_full", FailureClass::Busy),
            Passes::ByItself
        );
        assert_eq!(
            book.passes("on_hold", FailureClass::Busy),
            Passes::NotByItself
        );
        // A code the book lacks, or holds with another class: the class's default.
        assert_eq!(
            book.passes("lost_line", FailureClass::Transient),
            Passes::ByItself
        );
        assert_eq!(
            book.passes("queue_full", FailureClass::Policy),
            Passes::NotByItself
        );
        assert_eq!("by_itself".parse::<Passes>().unwrap(), Passes::ByItself);
    }
}
