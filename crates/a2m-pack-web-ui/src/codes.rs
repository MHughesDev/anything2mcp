//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest. `timeout`,
//! `connection_lost`, and `no_confirmation` are the provider contract's own.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::StepFailure;

/// `drift`: no locator of a field, an action, or a table matched.
pub const LOCATOR_NOT_FOUND: FailureCode = FailureCode::fixed("locator_not_found");
/// `drift`: a locator matched more than one element. The first is never
/// picked.
pub const LOCATOR_AMBIGUOUS: FailureCode = FailureCode::fixed("locator_ambiguous");
/// `drift`: the element found is not where its fingerprint says, or a
/// table's header is not the one its screen declares.
pub const FINGERPRINT_MISMATCH: FailureCode = FailureCode::fixed("fingerprint_mismatch");
/// `unknown_variant`: a dialog of the page that no screen declares.
pub const UNEXPECTED_DIALOG: FailureCode = FailureCode::fixed("unexpected_dialog");
/// `unknown_variant`: the page is not in a state its screen declares.
pub const UNEXPECTED_PAGE: FailureCode = FailureCode::fixed("unexpected_page");
/// `unknown_variant`: the element found is not a control its field's rule
/// can set or read.
pub const UNEXPECTED_CONTROL: FailureCode = FailureCode::fixed("unexpected_control");
/// `unknown_variant`: the plan names a control this driver has no rule for.
pub const UNKNOWN_CONTROL: FailureCode = FailureCode::fixed("unknown_control");
/// `unknown_variant`: the program that started is not a browser the engine
/// can own. The browser engine's code.
pub const BROWSER_NOT_OWNED: FailureCode = FailureCode::fixed("browser_not_owned");
/// `unknown_variant`: what a read gave is larger than the limit.
pub const READ_TOO_LARGE: FailureCode = FailureCode::fixed("read_too_large");
/// `unknown_variant`: a read from inside the page was answered with a
/// status it does not declare.
pub const UNEXPECTED_STATUS: FailureCode = FailureCode::fixed("unexpected_status");
/// `drift`: the answer to a read from inside the page is not the shape its
/// entry declares, or not of its media type.
pub const RESPONSE_SCHEMA_MISMATCH: FailureCode = FailureCode::fixed("response_schema_mismatch");
/// `rejected`: the application showed a declared refusal.
pub const APP_VALIDATION_ERROR: FailureCode = FailureCode::fixed("app_validation_error");
/// `policy`: a page address outside the surfaces the run may reach.
pub const SURFACE_NOT_DECLARED: FailureCode = FailureCode::fixed("surface_not_declared");
/// `policy`: the page sent a request that could change the system while a
/// read ran, and the tab, which cannot write, failed it (decision 0195).
pub const PAGE_WRITE_REFUSED: FailureCode = FailureCode::fixed("page_write_refused");
/// `policy`: a password input was asked for its value. The browser
/// engine's code.
pub const PASSWORD_READ_REFUSED: FailureCode = FailureCode::fixed("password_read_refused");
/// `busy`: no browser is installed. The browser engine's code.
pub const BROWSER_UNAVAILABLE: FailureCode = FailureCode::fixed("browser_unavailable");
/// `input`: a step this driver cannot carry out as the plan wrote it.
pub const STEP_NOT_USABLE: FailureCode = FailureCode::fixed("step_not_usable");
/// `input`: an address a tab may not be sent to. The browser engine's code.
pub const ADDRESS_INVALID: FailureCode = FailureCode::fixed("address_invalid");
/// `input`: a value that cannot stand in one segment of a screen's address.
pub const ADDRESS_VALUE_REFUSED: FailureCode = FailureCode::fixed("address_value_refused");

/// `drift`: the page a write is made from holds no anti-forgery token
/// where its entry says. Nothing was sent (decision 0206).
pub const ANTI_FORGERY_NOT_FOUND: FailureCode = FailureCode::fixed("anti_forgery_not_found");
/// `unknown_variant`: the page holds two different anti-forgery tokens
/// where its entry says, or one of a form that is not sent. Nothing was
/// sent.
pub const ANTI_FORGERY_UNREADABLE: FailureCode = FailureCode::fixed("anti_forgery_unreadable");

/// The sign-in page of a form sign-in shows the block the target declares:
/// a locked account, a check the engine does not get past (decision 0245).
pub const SIGN_IN_BLOCKED: FailureCode = FailureCode::fixed("sign_in_blocked");

/// Every code of this pack with its one class, as the manifest declares
/// them. A test holds the two lists together.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Drift, "locator_not_found"),
    (FailureClass::Drift, "locator_ambiguous"),
    (FailureClass::Drift, "fingerprint_mismatch"),
    (FailureClass::Drift, "response_schema_mismatch"),
    (FailureClass::Drift, "anti_forgery_not_found"),
    (FailureClass::UnknownVariant, "anti_forgery_unreadable"),
    (FailureClass::UnknownVariant, "unexpected_dialog"),
    (FailureClass::UnknownVariant, "unexpected_page"),
    (FailureClass::UnknownVariant, "unexpected_control"),
    (FailureClass::UnknownVariant, "unknown_control"),
    (FailureClass::UnknownVariant, "browser_not_owned"),
    (FailureClass::UnknownVariant, "read_too_large"),
    (FailureClass::UnknownVariant, "unexpected_status"),
    (FailureClass::Rejected, "app_validation_error"),
    (FailureClass::Policy, "surface_not_declared"),
    (FailureClass::Policy, "page_write_refused"),
    (FailureClass::Policy, "password_read_refused"),
    (FailureClass::Busy, "browser_unavailable"),
    (FailureClass::Input, "step_not_usable"),
    (FailureClass::Input, "address_invalid"),
    (FailureClass::Input, "address_value_refused"),
    (FailureClass::EdgeBlocked, "sign_in_blocked"),
];

pub(crate) fn failure(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: &str,
) -> Failure {
    Failure::new(class, code, message, next)
}

/// A step the driver cannot carry out as written. Nothing was sent.
pub(crate) fn not_usable(why: impl Into<String>) -> StepFailure {
    StepFailure::not_sent(failure(
        FailureClass::Input,
        STEP_NOT_USABLE,
        format!(
            "The interface driver cannot carry out this step as the plan wrote it: {}.",
            why.into()
        ),
        "Compile the target again with the version of the web pack that is enabled.",
    ))
}

pub(crate) fn surface_not_declared(what: &str) -> Failure {
    failure(
        FailureClass::Policy,
        SURFACE_NOT_DECLARED,
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
