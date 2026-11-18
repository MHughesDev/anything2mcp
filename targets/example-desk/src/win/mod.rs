//! The stand's own part on Windows: the window, made of plain Win32
//! controls (`window`), and the check that this process can show one that
//! UI Automation reads. The harness that starts and stops the stand's
//! process is shared with macOS (`crate::child`).

pub(crate) mod window;

/// Whether this process's window station is the interactive one: whether
/// a window it shows is on a desktop UI Automation reads. A service, or a
/// runner with no signed-in session, has none.
#[allow(unsafe_code)] // two calls into the system, explained below
fn interactive() -> Result<bool, String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::WSF_VISIBLE;
    // SAFETY: no argument; the handle the call gives is not to be closed.
    let station = unsafe { GetProcessWindowStation() }
        .map_err(|error| format!("GetProcessWindowStation: {}", error.message()))?;
    let mut flags = USEROBJECTFLAGS::default();
    let mut needed = 0_u32;
    // SAFETY: `flags` is ours and as long as the length says.
    unsafe {
        GetUserObjectInformationW(
            HANDLE(station.0),
            UOI_FLAGS,
            Some((&raw mut flags).cast()),
            size_of::<USEROBJECTFLAGS>() as u32,
            Some(&raw mut needed),
        )
    }
    .map_err(|error| format!("GetUserObjectInformationW: {}", error.message()))?;
    Ok(flags.dwFlags & WSF_VISIBLE as u32 != 0)
}

/// Why this process cannot give the stand a desktop, or `None` when it
/// can.
pub(crate) fn missing() -> Result<Option<String>, String> {
    Ok((!interactive()?).then(|| {
        "this process's window station is not the interactive one, so no window it shows can be read".to_owned()
    }))
}
