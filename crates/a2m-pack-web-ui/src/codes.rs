//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest; a test holds the two lists
//! together. `timeout`, `connection_lost`, `no_confirmation`, and
//! `read_only_posture` are the provider contract's own, and the sign-in's
//! refusals are the session keeper's; the manifest declares none of them.
//! The browser engine's codes that the pack gives as they are, and the
//! manifest declares too, are in [`PASSED_ON`].
//!
//! The pack has no `busy` code of its own; the browser engine's
//! `browser_unavailable` does not pass by itself.

use a2m_core::{CodeEntry, Failure, FailureCode};
use a2m_provider::StepFailure;

a2m_core::failure_codes! {
    LOCATOR_NOT_FOUND = Drift, "locator_not_found",
        meaning "No locator of a field, an action, or a table matched.",
        next "Look at the page and correct the screen's locators.";
    LOCATOR_AMBIGUOUS = Drift, "locator_ambiguous",
        meaning "A locator matched more than one element. The first is never picked.",
        next "Make the locator more exact; the first of several is never picked.";
    FINGERPRINT_MISMATCH = Drift, "fingerprint_mismatch",
        meaning "The element found is not where its fingerprint says, or a table's header is not the one its screen declares.",
        next "Look at the page and correct the screen, or the `header` of its `rows`; the element may have moved, or another may have taken its name.";
    RESPONSE_SCHEMA_MISMATCH = Drift, "response_schema_mismatch",
        meaning "The answer to a read from inside the page is not the shape its entry declares, or not of its media type.",
        next "Look at what the application answers now, and correct the entry under `page_requests/`.";
    ANTI_FORGERY_NOT_FOUND = Drift, "anti_forgery_not_found",
        meaning "The page a write is made from holds no anti-forgery token where its entry says. Nothing was sent (decision 0206).",
        next "Look at where the page holds its token now, and correct the entry's `anti_forgery`.";
    ANTI_FORGERY_UNREADABLE = UnknownVariant, "anti_forgery_unreadable",
        meaning "The page holds two different anti-forgery tokens where its entry says, or one of a form that is not sent. Nothing was sent.",
        next "Look at the page; a token is read from one place that holds one text.";
    UNEXPECTED_DIALOG = UnknownVariant, "unexpected_dialog",
        meaning "A dialog of the page that no screen declares.",
        next "Look at the page; a dialog is declared before a run may answer it.";
    UNEXPECTED_PAGE = UnknownVariant, "unexpected_page",
        meaning "The page is not in a state its screen declares.",
        next "Look at the page; declare the state it is in as a variant of the screen.";
    UNEXPECTED_CONTROL = UnknownVariant, "unexpected_control",
        meaning "The element found is not a control its field's rule can set or read.",
        next "Look at the page; correct the field's `control`, or declare the state the page is in.";
    UNKNOWN_CONTROL = UnknownVariant, "unknown_control",
        meaning "The plan names a control this driver has no rule for.",
        next "Compile the target again with the version of the web pack that is enabled.";
    READ_TOO_LARGE = UnknownVariant, "read_too_large",
        meaning "What a read gave is larger than the limit.",
        next "Raise `packs.web.max_read_chars`, or read a smaller part of the page.";
    UNEXPECTED_STATUS = UnknownVariant, "unexpected_status",
        meaning "A read from inside the page was answered with a status it does not declare.",
        next "Look at what the application answers; the entry declares the statuses it reads.";
    APP_VALIDATION_ERROR = Rejected, "app_validation_error",
        meaning "The application showed a declared refusal.",
        next "Correct the values and run the tool again; the application refused them.";
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared",
        meaning "A page address outside the surfaces the run may reach.",
        next "Declare the surface in the target's files if the system really lives there; otherwise look at why the page moved.";
    PAGE_WRITE_REFUSED = Policy, "page_write_refused",
        meaning "The page sent a request that could change the system while a read ran, and the tab, which cannot write, failed it (decision 0195).",
        next "Look at what the page does when it loads; a page that writes by itself is not read by the web pack, so read the record another way.";
    STEP_NOT_USABLE = Input, "step_not_usable",
        meaning "A step this driver cannot carry out as the plan wrote it.",
        next "Compile the target again with the version of the web pack that is enabled.";
    SIGN_IN_BLOCKED = EdgeBlocked, "sign_in_blocked",
        meaning "The sign-in page of a form sign-in shows the block the target declares: a locked account, a check the engine does not get past (decision 0245).",
        next "Look at the application in a browser of your own; sign in again once the block is lifted.";
}

/// The browser engine's: no browser is installed.
pub const BROWSER_UNAVAILABLE: FailureCode = a2m_pack_web_browser::codes::BROWSER_UNAVAILABLE;
/// The browser engine's: the program that started is not a browser the
/// engine can own.
pub const BROWSER_NOT_OWNED: FailureCode = a2m_pack_web_browser::codes::BROWSER_NOT_OWNED;
/// The browser engine's: a password input was asked for its value.
pub const PASSWORD_READ_REFUSED: FailureCode = a2m_pack_web_browser::codes::PASSWORD_READ_REFUSED;
/// The browser engine's: an address a tab may not be sent to.
pub const ADDRESS_INVALID: FailureCode = a2m_pack_web_browser::codes::ADDRESS_INVALID;
/// The browser engine's: a value that cannot stand in one segment of an
/// address.
pub const ADDRESS_VALUE_REFUSED: FailureCode = a2m_pack_web_browser::codes::ADDRESS_VALUE_REFUSED;

/// The codes of the browser engine, `a2m_pack_web_browser::codes`, that
/// the pack gives as they are, or makes in the engine's class, and the
/// manifest declares too.
pub const PASSED_ON: &[FailureCode] = &[
    BROWSER_UNAVAILABLE,
    BROWSER_NOT_OWNED,
    PASSWORD_READ_REFUSED,
    ADDRESS_INVALID,
    ADDRESS_VALUE_REFUSED,
];

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(row: &CodeEntry, message: impl Into<String>, next: &str) -> Failure {
    row.failure(message, next)
}

/// A step the driver cannot carry out as written. Nothing was sent.
pub(crate) fn not_usable(why: impl Into<String>) -> StepFailure {
    StepFailure::not_sent(failure(
        &row::STEP_NOT_USABLE,
        format!(
            "The interface driver cannot carry out this step as the plan wrote it: {}.",
            why.into()
        ),
        "Compile the target again with the version of the web pack that is enabled.",
    ))
}

pub(crate) fn surface_not_declared(what: &str) -> Failure {
    failure(
        &row::SURFACE_NOT_DECLARED,
        format!("{what} is not one of the addresses this run may reach."),
        "Declare the surface in `target.yaml`, or the destination in the operation's `permissions`.",
    )
}

/// Text from a target file or a page, cut and cleaned so it can stand in a
/// message.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 80;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No source of this crate gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if name != "tests" {
                        walk(&path, found);
                    }
                } else if name.ends_with(".rs") && name != "tests.rs" {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let text = text.split("#[cfg(test)]").next().unwrap();
                    for made in ["Failure::new(", "class: FailureClass::"] {
                        if text.contains(made) {
                            found.push(format!("{}: {made}", path.display()));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:?}"
        );
        for row in TABLE {
            let made = failure(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
