//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest. `timeout` and
//! `connection_lost` are the provider contract's own.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::StepFailure;

/// `drift`: no locator of a field or a table matched.
pub const LOCATOR_NOT_FOUND: FailureCode = FailureCode::fixed("locator_not_found");
/// `drift`: a locator matched more than one element, or a screen's window
/// matched more than one window. The first is never picked.
pub const LOCATOR_AMBIGUOUS: FailureCode = FailureCode::fixed("locator_ambiguous");
/// `drift`: a table's headings are not the ones its screen declares, or a
/// row of it does not have one cell for each heading.
pub const FINGERPRINT_MISMATCH: FailureCode = FailureCode::fixed("fingerprint_mismatch");
/// `unknown_variant`: the window a screen declares did not appear, or did
/// not become ready, by the step's deadline.
pub const UNEXPECTED_PAGE: FailureCode = FailureCode::fixed("unexpected_page");
/// `unknown_variant`: more than one running application is the program
/// and the name the surface states.
pub const APPLICATION_AMBIGUOUS: FailureCode = FailureCode::fixed("application_ambiguous");
/// `unknown_variant`: the running application's program is not the one
/// whose digest the surface states.
pub const APPLICATION_NOT_PROVEN: FailureCode = FailureCode::fixed("application_not_proven");
/// `unknown_variant`: a value is longer than the pack reads.
pub const READ_TOO_LARGE: FailureCode = FailureCode::fixed("read_too_large");
/// `policy`: a step would read the value of a control that holds a secret.
pub const PASSWORD_READ_REFUSED: FailureCode = FailureCode::fixed("password_read_refused");
/// `busy`: no running application is the program and the name the surface
/// states. The pack starts nothing.
pub const APPLICATION_NOT_RUNNING: FailureCode = FailureCode::fixed("application_not_running");
/// `busy`: the session's accessibility switch is off and the application
/// publishes no tree. The pack changes no setting.
pub const ACCESSIBILITY_OFF: FailureCode = FailureCode::fixed("accessibility_off");
/// `busy`: the application runs at a higher level than the engine, as
/// administrator. Windows only; the pack does not reach it.
pub const TARGET_ELEVATED: FailureCode = FailureCode::fixed("target_elevated");
/// `policy`: the program the surface states runs as another user, or in
/// another session. Windows only; the pack reads none of it.
pub const APPLICATION_NOT_OWNED: FailureCode = FailureCode::fixed("application_not_owned");
/// `busy`: this pack has no back end for the operating system it runs on.
pub const SYSTEM_NOT_BUILT: FailureCode = FailureCode::fixed("system_not_built");
/// `input`: a step this driver cannot carry out as the plan wrote it.
pub const STEP_NOT_USABLE: FailureCode = FailureCode::fixed("step_not_usable");

/// Every code of this pack with its one class, as the manifest declares
/// them. A test holds the two lists together.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Drift, "locator_not_found"),
    (FailureClass::Drift, "locator_ambiguous"),
    (FailureClass::Drift, "fingerprint_mismatch"),
    (FailureClass::UnknownVariant, "unexpected_page"),
    (FailureClass::UnknownVariant, "application_ambiguous"),
    (FailureClass::UnknownVariant, "application_not_proven"),
    (FailureClass::UnknownVariant, "read_too_large"),
    (FailureClass::Policy, "password_read_refused"),
    (FailureClass::Policy, "application_not_owned"),
    (FailureClass::Busy, "application_not_running"),
    (FailureClass::Busy, "accessibility_off"),
    (FailureClass::Busy, "target_elevated"),
    (FailureClass::Busy, "system_not_built"),
    (FailureClass::Input, "step_not_usable"),
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
            "The desktop driver cannot carry out this step as the plan wrote it: {}.",
            why.into()
        ),
        "Compile the target again with the version of the desktop pack that is enabled.",
    ))
}

/// The pack has no back end for this operating system.
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn system_not_built() -> Failure {
    failure(
        FailureClass::Busy,
        SYSTEM_NOT_BUILT,
        format!(
            "The desktop pack reads applications only on Linux and Windows in this version; this system is {}.",
            std::env::consts::OS
        ),
        "Run the target on Linux or Windows. The macOS back end is not built yet.",
    )
}

/// Text from a target file or an application, cut and cleaned so it can
/// stand in a message.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 80;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() && !a2m_core::said_hides(*c))
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}
