//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest; a test holds the two lists
//! together. `timeout`, `connection_lost`, `no_confirmation`, and
//! `read_only_posture` are the provider contract's own, and the sign-in's
//! refusals are the session keeper's; the manifest declares none of them.
//! The browser engine's codes that the pack gives as they are, and the
//! manifest declares too, are in [`PASSED_ON`].
//!
//! The pack has no `busy` code of its own; the browser engine's
//! `browser_unavailable` and `browser_stopped_at_start` do not pass by
//! themselves, and its `memory_short`
//! does (decision 0425).

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
    COMMIT_NOT_REDRAWN = UnknownVariant, "commit_not_redrawn",
        meaning "A value was committed on a surface whose platform profile says the page sends a change and draws the form again, and the page did not draw it again in the time given. What became of the value is not known.",
        next "Look at the page and find out what the application holds; then correct the surface's `profile`, or the screen, before running the tool again.";
    UNKNOWN_CONTROL = UnknownVariant, "unknown_control",
        meaning "The compiled target names a control this driver has no rule for.",
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
        meaning "The page sent a request that could change the system while a read ran, and the tab, which cannot write, failed it (decision 0195); or, in the tab a write is made in, the page made a socket or a worker the tab refuses.",
        next "Look at what the page does when it loads. When a page request asserts the request with `read_asserted: true`, run the tool from a published version, live or in an environment of kind `test` (decision 0394); a page that writes by itself is not read by the web pack, so read the record another way.";
    ORIGIN_NOT_DECLARED = Policy, "origin_not_declared",
        meaning "The page, a frame of it, or a worker sent a request to an origin that is neither the surface's nor one the operation declares. The browser failed it before it left (decision 0292).",
        next "Declare the origin in the operation's `permissions.destinations`, with its scheme, if the system's pages really load from it; otherwise look at why the page sends there.";
    CHALLENGE_PAGE = EdgeBlocked, "challenge_page",
        meaning "The edge answered with a challenge that asks the visitor to prove it is not a program. The engine does not answer a challenge, and the surface is left alone for a while.",
        next "Ask the application's owner or its IT to allow the engine's address; a person may open the application in a browser of their own.";
    RATE_LIMITED_BY_EDGE = EdgeBlocked, "rate_limited_by_edge",
        meaning "The surface answered a page with status 429.",
        next "Look at the surface in a browser, and ask its owner before trying again.";
    FORBIDDEN_BY_EDGE = EdgeBlocked, "forbidden_by_edge",
        meaning "The surface answered a page with status 403.",
        next "Look at the surface in a browser, and ask its owner before trying again.";
    SURFACE_IN_MAINTENANCE = Busy, "surface_in_maintenance",
        meaning "The surface answered 503: it is being worked on. Nothing is retried, and the surface is left alone for a while.",
        next "Try again after the maintenance ends.",
        passes by_itself;
    SITE_TOOL_NOT_OFFERED = Drift, "site_tool_not_offered",
        meaning "The page registered no tool of the name the binding calls, in the time given.",
        next "Look at which tools the page offers now; the page may have stopped offering the tool, or the binding names another.";
    SITE_TOOL_CHANGED = Drift, "site_tool_changed",
        meaning "The tool the page registered under the name is not the tool the binding was written for: its description or its input schema differs from the digest the binding holds. Nothing was sent.",
        next "Look at the tool the page offers now, and, if it is the one meant, pin the digest this message gives in the binding.";
    SITE_TOOLS_UNAVAILABLE = UnknownVariant, "site_tools_unavailable",
        meaning "The browser has no tools of pages to offer: the browser the engine owns does not carry the experimental domain they are called through, or a page of it has no place to register one (`document.modelContext`), as a page that is not a secure context has none.",
        next "Use a browser of a version that carries it, or bind the operation through the interface.";
    DOWNLOAD_NOT_OFFERED = UnknownVariant, "download_not_offered",
        meaning "The action declares a download, and the page began none before the step's time ended.",
        next "Look at the page; the action's click does not offer a file, or the file is offered another way than a download.";
    DOWNLOAD_TOO_LARGE = UnknownVariant, "download_too_large",
        meaning "The file the page offered is larger than the most the action declares. It was cancelled and nothing was kept.",
        next "Raise `download.most_bytes` of the action if the file is the one meant; otherwise look at what the page offers.";
    DOWNLOAD_INCOMPLETE = UnknownVariant, "download_incomplete",
        meaning "The browser ended the download before the file was whole, or the step's time ended before it was. Nothing was kept.",
        next "Run the tool again; if it ends the same way, look at the page in a browser of your own.";
    FILE_NOT_STAGED = Input, "file_not_staged",
        meaning "A file input was given a name that is not the name of a file in the staging folder: it holds no file of that name, or the name is not one plain name.",
        next "Put the file in the staging folder under the name, and give that name; no path is taken.";
    STEP_NOT_USABLE = Input, "step_not_usable",
        meaning "A step this driver cannot carry out as the compiled target wrote it.",
        next "Compile the target again with the version of the web pack that is enabled.";
    SIGN_IN_BLOCKED = EdgeBlocked, "sign_in_blocked",
        meaning "The sign-in page of a form sign-in shows the block the target declares: a locked account, a check the engine does not get past (decision 0245).",
        next "Look at the application in a browser of your own; sign in again once the block is lifted.";
}

/// The browser engine's: no browser is installed.
pub const BROWSER_UNAVAILABLE: FailureCode = a2m_pack_web_browser::codes::BROWSER_UNAVAILABLE;
/// The browser engine's: memory runs short, so no new browser work is
/// started; the failure carries a retry time.
pub const MEMORY_SHORT: FailureCode = a2m_pack_web_browser::codes::MEMORY_SHORT;
/// The browser engine's: the program that started is not a browser the
/// engine can own.
pub const BROWSER_NOT_OWNED: FailureCode = a2m_pack_web_browser::codes::BROWSER_NOT_OWNED;
/// The browser engine's: the browser stopped as it started, and wrote why.
pub const BROWSER_STOPPED_AT_START: FailureCode =
    a2m_pack_web_browser::codes::BROWSER_STOPPED_AT_START;
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
    MEMORY_SHORT,
    BROWSER_NOT_OWNED,
    BROWSER_STOPPED_AT_START,
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
            "The interface driver cannot carry out this step as the compiled target wrote it: {}.",
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
        let found = a2m_core::testing::classes_of_their_own(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &[],
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:#?}"
        );
        for row in TABLE {
            let made = failure(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
