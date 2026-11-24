//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest; a test holds the two lists
//! together. `timeout`, `connection_lost`, `no_confirmation`, and
//! `read_only_posture` are the provider contract's own, which the driver
//! returns and the manifest does not declare; [`PASSED_ON`] names the codes
//! of other crates the manifest declares, and there are none.
//!
//! Whether a `busy` code passes by itself, and why: `session_in_use` does,
//! because the other write that holds the desktop ends by its own deadline.
//! The others do not: a person must start the application, turn on or allow
//! accessibility, start it without administrator rights, or run the target
//! on a system the pack is built for.

use a2m_core::{CodeEntry, Failure, FailureCode};
use a2m_provider::StepFailure;

a2m_core::failure_codes! {
    LOCATOR_NOT_FOUND = Drift, "locator_not_found",
        meaning "No locator of a field or a table matched.",
        next "Take a snapshot of the window and correct the screen's locators.";
    LOCATOR_AMBIGUOUS = Drift, "locator_ambiguous",
        meaning "A locator matched more than one element, or a screen's window matched more than one window. The first is never picked.",
        next "Give the screen a locator the application holds once, or leave the field out.";
    FINGERPRINT_MISMATCH = Drift, "fingerprint_mismatch",
        meaning "A table's headings are not the ones its screen declares, or a row of it does not have one cell for each heading.",
        next "Take a snapshot of the window and correct the screen's `header`.";
    UNEXPECTED_PAGE = UnknownVariant, "unexpected_page",
        meaning "The window a screen declares did not appear, or did not become ready, by the step's deadline.",
        next "Check that the application shows the window the screen declares, then run the tool again.";
    APPLICATION_AMBIGUOUS = UnknownVariant, "application_ambiguous",
        meaning "More than one running application is the program and the name the surface states.",
        next "Close the other copies of the application, then run the tool again.";
    APPLICATION_NOT_PROVEN = UnknownVariant, "application_not_proven",
        meaning "The running application's program is not the one whose digest the surface states.",
        next "Review the program that is installed, then state its digest in the surface, or reinstall the reviewed one.";
    READ_TOO_LARGE = UnknownVariant, "read_too_large",
        meaning "A value is longer than the pack reads.",
        next "Read a field that holds less, or a smaller window.";
    PASSWORD_READ_REFUSED = Policy, "password_read_refused",
        meaning "A step would read the value of a control that holds a secret.",
        next "Read another field. A secret is never an output of a tool.";
    APPLICATION_NOT_OWNED = Policy, "application_not_owned",
        meaning "The program the surface states runs as another user, or on Windows in another session. Windows and macOS; the pack reads none of it.",
        next "Start the application in your own session, as yourself, then run the tool again.";
    APPLICATION_NOT_RUNNING = Busy, "application_not_running",
        meaning "No running application is the program and the name the surface states. The pack starts nothing.",
        next "Start the application as you normally do, then run the tool again.",
        passes not_by_itself;
    ACCESSIBILITY_OFF = Busy, "accessibility_off",
        meaning "The session's accessibility switch is off and the application publishes no tree (Linux), or the person has not allowed this program to use the system's accessibility interface (macOS). The pack changes no setting.",
        next "Turn on the desktop's accessibility setting, or allow the program that started the engine to use the system's accessibility interface, then run the tool again.",
        passes not_by_itself;
    TARGET_ELEVATED = Busy, "target_elevated",
        meaning "The application runs at a higher level than the engine, as administrator. Windows only; the pack does not reach it.",
        next "Start the application as yourself, without administrator rights, then run the tool again.",
        passes not_by_itself;
    SYSTEM_NOT_BUILT = Busy, "system_not_built",
        meaning "This pack has no back end for the operating system it runs on.",
        next "Run the target on Linux, Windows, or macOS.",
        passes not_by_itself;
    SERVICE_NOT_RUNNING = Busy, "service_not_running",
        meaning "No connection owns the bus name the surface states as its service (Linux). The pack starts nothing, whatever a `.service` file says.",
        next "Start the application as you normally do, then run the tool again.",
        passes not_by_itself;
    PROPERTY_NOT_TEXT = UnknownVariant, "property_not_text",
        meaning "A service's property holds a value that is not text, a flag, a number, or an object path. The pack gives no other form.",
        next "Read another property.";
    DICTIONARY_INCLUDE_REFUSED = Policy, "dictionary_include_refused",
        meaning "A scripting dictionary a person handed the scout includes a file outside the application's bundle, which the pack never reads. The dictionary is refused whole.",
        next "Hand a dictionary that includes nothing outside its bundle, or leave it out of the discovery.";
    TYPE_LIBRARY_FORMAT_UNKNOWN = UnknownVariant, "type_library_format_unknown",
        meaning "A file handed as a COM type library is in a form the reader does not know: the older form, a program file that may hold one, or something else. Only the MSFT form is read, and none is loaded by the system.",
        next "Hand a type library in the MSFT form, or leave it out of the discovery.";
    STEP_NOT_BUILT = Busy, "step_not_built",
        meaning "A read of a management class's property or of a COM property get was drafted and compiles, and this version of the pack carries out no such step.",
        next "Use the read once a version of the desktop pack that carries out the step is enabled.",
        passes not_by_itself;
    STEP_NOT_USABLE = Input, "step_not_usable",
        meaning "A step this driver cannot carry out as the compiled target wrote it, or a snapshot of the driver's that is not in the form the pack's discoverer reads.",
        next "Compile the target again with the version of the desktop pack that is enabled.";
    APP_VALIDATION_ERROR = Rejected, "app_validation_error",
        meaning "After a committing action, the window shows a state its screen declares as the application's refusal. Nothing was stored.",
        next "Correct the values and run the tool again; the application refused them.";
    FIELD_READ_ONLY = UnknownVariant, "field_read_only",
        meaning "A field takes no value today: it is read-only, or not editable. Nothing was set.",
        next "Look at the application; the field may be locked for this record.";
    VALUE_NOT_HELD = UnknownVariant, "value_not_held",
        meaning "After a `set`, the field holds another value than the one set: the application changed it. Nothing was saved.",
        next "Give a value the field holds as it is written, such as a shorter one.";
    UNEXPECTED_CONTROL = UnknownVariant, "unexpected_control",
        meaning "The element does not offer what the step needs: an editable text, or an action of the name the screen declares.",
        next "Take a snapshot of the window and correct the screen's field or the action's `perform`.";
    PASSWORD_SET_REFUSED = Policy, "password_set_refused",
        meaning "A step would set a control that holds a secret. Sign-in to a desktop application is not built.",
        next "Leave the field out. Signing in to a desktop application is not built in this version.";
    SESSION_IN_USE = Busy, "session_in_use",
        meaning "Another write holds this engine's desktop past the step's deadline. Nothing was sent.",
        next "Run the tool again once the other write has ended.",
        passes by_itself;
    WRITES_NOT_BUILT = Busy, "writes_not_built",
        meaning "This pack writes only on Linux, Windows, and macOS in this version. Nothing was sent.",
        next "Run the write on Linux, Windows, or macOS. On this system the pack reads only.",
        passes not_by_itself;
    REMOTE_NAMESPACE_REFUSED = Policy, "remote_namespace_refused",
        meaning "The surface's management namespace names another machine (it begins with two separators). The driver asks only this machine's management service, and asks nothing of it.",
        next "Leave `management_namespace` out, or name a namespace of this machine.";
}

/// The codes of other crates the driver returns as they are, which the
/// manifest declares too. There are none: the provider contract's codes the
/// driver returns are the contract's, and the manifest does not declare
/// them.
pub const PASSED_ON: &[FailureCode] = &[];

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(row: &CodeEntry, message: impl Into<String>, next: &str) -> Failure {
    row.failure(message, next)
}

/// A step the driver cannot carry out as written. Nothing was sent.
pub(crate) fn not_usable(why: impl Into<String>) -> StepFailure {
    StepFailure::not_sent(failure(
        &row::STEP_NOT_USABLE,
        format!(
            "The desktop driver cannot carry out this step as the compiled target wrote it: {}.",
            why.into()
        ),
        "Compile the target again with the version of the desktop pack that is enabled.",
    ))
}

/// A write on a system where the pack's acting half is not built. Nothing
/// was sent.
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub(crate) fn writes_not_built() -> StepFailure {
    StepFailure::not_sent(failure(
        &row::WRITES_NOT_BUILT,
        format!(
            "The desktop pack changes applications only on Linux, Windows, and macOS in this version; this system is {}. Nothing was sent.",
            std::env::consts::OS
        ),
        "Run the write on Linux, Windows, or macOS. On this system the pack reads only.",
    ))
}

/// A management namespace of another machine, refused by the driver before
/// the management service is asked anything.
#[cfg(windows)]
pub(crate) fn remote_namespace_refused() -> StepFailure {
    StepFailure::not_sent(failure(
        &row::REMOTE_NAMESPACE_REFUSED,
        "The surface's management namespace names another machine; the desktop pack asks only this machine's management service. Nothing was sent.".to_owned(),
        "Leave `management_namespace` out, or name a namespace of this machine.",
    ))
}

/// A service of the session bus asked on a system other than Linux. Nothing
/// was sent.
#[cfg(not(target_os = "linux"))]
pub(crate) fn service_not_built() -> StepFailure {
    StepFailure::not_sent(failure(
        &row::SYSTEM_NOT_BUILT,
        format!(
            "The desktop pack reads the services of the session bus only on Linux in this version; this system is {}. Nothing was sent.",
            std::env::consts::OS
        ),
        "Run the target on Linux.",
    ))
}

/// A management class asked on a system other than Windows. Nothing was
/// sent.
#[cfg(not(windows))]
pub(crate) fn management_not_built() -> StepFailure {
    StepFailure::not_sent(failure(
        &row::SYSTEM_NOT_BUILT,
        format!(
            "The desktop pack reads the definitions of management classes only on Windows in this version; this system is {}. Nothing was sent.",
            std::env::consts::OS
        ),
        "Run the target on Windows.",
    ))
}

/// A drafted read that this version carries out no step for. Nothing was
/// sent.
pub(crate) fn step_not_built(kind: &str) -> StepFailure {
    StepFailure::not_sent(failure(
        &row::STEP_NOT_BUILT,
        format!(
            "This version of the desktop pack carries out no `{}` step: the read compiles, and is not run yet.",
            shown(kind)
        ),
        "Use the read once a version of the desktop pack that carries out the step is enabled.",
    ))
}

/// The pack has no back end for this operating system.
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub(crate) fn system_not_built() -> Failure {
    failure(
        &row::SYSTEM_NOT_BUILT,
        format!(
            "The desktop pack reads applications only on Linux, Windows, and macOS in this version; this system is {}.",
            std::env::consts::OS
        ),
        "Run the target on Linux, Windows, or macOS.",
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
