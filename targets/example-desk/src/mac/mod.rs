//! The stand's own part on macOS: the window, made of AppKit's own
//! controls (`window`), and the check that a client in this process may
//! read it. The harness that starts and stops the stand's process is
//! shared with Windows (`crate::child`).

pub(crate) mod window;

/// Why this process cannot read the stand, or `None` when it can: the
/// person, or on a CI machine the step that grants it, has not allowed this
/// program to use the system's accessibility interface. The check never
/// shows the system's prompt.
#[allow(unsafe_code)] // one call into the system, explained below
pub(crate) fn missing() -> Option<String> {
    // SAFETY: no argument; the call only reads this process's permission.
    let trusted = unsafe { objc2_application_services::AXIsProcessTrusted() };
    if trusted {
        return None;
    }
    let program = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "this test program".to_owned());
    Some(format!(
        "this process ({program}) may not use the system's accessibility interface: it is not allowed under Privacy & Security, Accessibility, for itself or for the program that started it. On a CI machine this means the step that grants the permission did not take for this program"
    ))
}
