//! The presence prompt of Windows: Windows Hello.
//!
//! The engine asks Windows Hello, through `UserConsentVerifier`, to
//! verify the person: a face, a fingerprint, or the Hello PIN. A desktop
//! program that is not packaged asks through the class's interop,
//! `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`,
//! which needs a window to own the dialog. The engine has none, so each
//! ask makes a small window of its own, hidden, on a thread of its own
//! that runs the window's messages, and closes it when the ask ends. The
//! system draws the dialog and checks the answer; the engine gets one of
//! seven results.
//!
//! Three things keep a yes worth something:
//!
//! - **Before anybody is asked, the session and Windows Hello are asked.**
//!   An engine in the services' session or in a window station that is not
//!   the interactive one, over remote desktop, or at a locked screen
//!   raises nothing; `CheckAvailabilityAsync` must then say `Available`.
//!   Each other answer is a named `not_available`.
//! - **An error is never a yes.** Every result and every failure maps to
//!   a named outcome that records nothing: [`placed`], [`standing`], and
//!   [`answered`] are total, and their tests walk every case. A result this
//!   build does not know is `check_failed`.
//! - **The deadline takes the prompt away**: the operation is cancelled
//!   when the ask's future is dropped, at the deadline or for any other
//!   reason, and then the window is closed.
//!
//! What it does not prove is in spec section 222: the answer is a value
//! in the engine's process, which a program running as the same user can
//! open and change, and a program that replaces the engine can raise a
//! real prompt with false words.
//!
//! The system is behind one small trait, [`Verifier`], so that the
//! mapping is tested with a made-up system on every operating system. The
//! real one is in the module `system`, compiled on Windows only.

use std::sync::Arc;
use std::time::Duration;

use a2m_actions::BoxFuture;
use tokio::time::Instant;

use super::{Availability, PresencePrompt, PresenceReason, PromptAnswer, PromptName, PromptText};

/// The longest a call that asks nobody may take. A call that raises the
/// prompt is bounded by its deadline instead.
const PATIENCE: Duration = Duration::from_secs(10);

/// The values of `UserConsentVerificationResult` and of
/// `UserConsentVerifierAvailability`, which share their numbers. A test on
/// Windows holds them to the system's.
pub(crate) mod code {
    /// `Verified`, and for availability `Available`.
    pub(crate) const VERIFIED: i32 = 0;
    pub(crate) const DEVICE_NOT_PRESENT: i32 = 1;
    pub(crate) const NOT_CONFIGURED_FOR_USER: i32 = 2;
    pub(crate) const DISABLED_BY_POLICY: i32 = 3;
    pub(crate) const DEVICE_BUSY: i32 = 4;
    /// A result of a verification only.
    pub(crate) const RETRIES_EXHAUSTED: i32 = 5;
    /// A result of a verification only.
    pub(crate) const CANCELED: i32 = 6;
}

/// Whether the session's screen is locked, as the session's information
/// says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lock {
    Locked,
    Unlocked,
    /// The system says it does not know.
    Unknown,
}

/// Where the engine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Session {
    /// The process is in session 0, the services' session, which shows
    /// nothing to anybody.
    pub(crate) services: bool,
    /// The process's window station is `WinSta0`, the only one whose
    /// windows reach a screen.
    pub(crate) interactive_station: bool,
    /// The session is a remote desktop session.
    pub(crate) remote: bool,
    /// Whether the screen is locked.
    pub(crate) lock: Lock,
}

/// A way the system failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// A call failed, with its error code: an `HRESULT`, or a Win32 error.
    Call(i32),
    /// The window that owns the dialog could not be made.
    NoWindow,
    /// No answer came: a thread could not start or ended, or a call that
    /// asks nobody took too long.
    Lost,
}

/// Windows Hello and the session, as far as the prompt needs them.
pub(crate) trait Verifier: Send + Sync {
    /// Where the engine runs.
    fn session(&self) -> BoxFuture<'_, Result<Session, Fault>>;

    /// `CheckAvailabilityAsync`: a `UserConsentVerifierAvailability`. It
    /// shows nothing.
    fn availability(&self) -> BoxFuture<'_, Result<i32, Fault>>;

    /// `RequestVerificationForWindowAsync` with `message`, which the
    /// dialog shows: a `UserConsentVerificationResult`. Dropping the future
    /// before it ends cancels the operation, which takes the dialog away.
    fn verify(&self, message: String) -> BoxFuture<'_, Result<i32, Fault>>;
}

fn failed(what: String) -> Availability {
    Availability::not_available(
        PresenceReason::CheckFailed,
        format!("The check with Windows Hello failed ({what})."),
        "Run `anything2mcp doctor run` again. If it stays, the event log of this machine says why.",
    )
}

/// What a fault means for a person who wants to approve.
pub(crate) fn unavailable(fault: &Fault) -> Availability {
    match *fault {
        // Shown as the code's bits, the way Windows writes an error.
        Fault::Call(error) => failed(format!("error {:#010x}", error.cast_unsigned())),
        Fault::NoWindow => failed("no window to own the prompt".to_owned()),
        Fault::Lost => failed("no answer".to_owned()),
    }
}

/// What the session means for a person who wants to approve: `Ok` only
/// for the interactive window station of a local session whose screen is
/// not locked.
pub(crate) fn placed(session: Result<Session, Fault>) -> Result<(), Availability> {
    let session = session.map_err(|fault| unavailable(&fault))?;
    if session.services || !session.interactive_station {
        return Err(Availability::not_available(
            PresenceReason::NoWindowStation,
            "The engine runs where nothing it shows reaches a screen: as a service, or in a window station of its own.",
            "Start the engine from your own sign-in session at the machine itself. A service cannot approve.",
        ));
    }
    if session.remote {
        return Err(Availability::not_available(
            PresenceReason::SessionRemote,
            "The engine runs in a remote desktop session, so a prompt would be answered somewhere other than at this machine.",
            "Approve at the machine itself, signed in at its own screen.",
        ));
    }
    match session.lock {
        Lock::Unlocked => Ok(()),
        Lock::Locked => Err(Availability::not_available(
            PresenceReason::SessionLocked,
            "The screen of this machine is locked, so no presence prompt is raised.",
            "Unlock the machine, then press again.",
        )),
        Lock::Unknown => Err(failed("the session's lock state is unknown".to_owned())),
    }
}

/// What a result means when Windows Hello cannot verify anybody: shared by
/// the availability and the verification, whose numbers are the same.
fn cannot_verify(result: i32) -> Availability {
    match result {
        code::DEVICE_NOT_PRESENT | code::NOT_CONFIGURED_FOR_USER => Availability::not_available(
            PresenceReason::NotSetUp,
            "Windows Hello is not set up for this account, so nothing can ask for your face, fingerprint, or PIN.",
            "Set up a Windows Hello PIN in Settings, Accounts, Sign-in options, then press again.",
        ),
        code::DISABLED_BY_POLICY => Availability::not_available(
            PresenceReason::DisabledByPolicy,
            "A policy of this machine turns Windows Hello off, so no presence prompt can be raised.",
            "Approve on a machine whose policy allows Windows Hello, or ask the machine's administrator.",
        ),
        code::DEVICE_BUSY => Availability::not_available(
            PresenceReason::DeviceBusy,
            "Windows Hello is busy with another check.",
            "Finish or close the other Windows Hello prompt, then press again.",
        ),
        other => failed(format!("result {other}")),
    }
}

/// What `CheckAvailabilityAsync` says: `Ok` only for `Available`.
pub(crate) fn standing(availability: Result<i32, Fault>) -> Result<(), Availability> {
    match availability.map_err(|fault| unavailable(&fault))? {
        code::VERIFIED => Ok(()),
        other => Err(cannot_verify(other)),
    }
}

/// What a verification says once it has ended. Only `Verified` is a
/// person's presence; every other result and every fault is not.
pub(crate) fn answered(answer: Result<i32, Fault>) -> PromptAnswer {
    match answer {
        Ok(code::VERIFIED) => PromptAnswer::Present(PromptName::WindowsHello),
        Ok(code::CANCELED) => PromptAnswer::Dismissed,
        Ok(code::RETRIES_EXHAUSTED) => PromptAnswer::Denied,
        Ok(other) => PromptAnswer::NotAvailable(cannot_verify(other)),
        Err(fault) => PromptAnswer::NotAvailable(unavailable(&fault)),
    }
}

/// The message the dialog shows: the engine's own words, exactly as
/// [`PromptText`] carries them.
pub(crate) fn message_for(text: &PromptText) -> String {
    format!("{}: {}", text.title, text.detail)
}

/// `step`, or `lost` when it takes longer than a call that asks nobody
/// may.
async fn bounded<T>(lost: T, step: impl Future<Output = T>) -> T {
    tokio::time::timeout(PATIENCE, step).await.unwrap_or(lost)
}

/// The presence prompt over Windows Hello.
pub(crate) struct WindowsHelloPrompt {
    system: Arc<dyn Verifier>,
}

impl WindowsHelloPrompt {
    /// The prompt over this machine's own Windows Hello.
    #[cfg(windows)]
    pub(crate) fn system() -> Self {
        Self::over(Arc::new(system::Hello))
    }

    /// The prompt over `system`.
    pub(crate) fn over(system: Arc<dyn Verifier>) -> Self {
        Self { system }
    }

    /// Asks the session and Windows Hello, without raising anything,
    /// whether a prompt can be raised.
    async fn ready(&self) -> Result<(), Availability> {
        placed(bounded(Err(Fault::Lost), self.system.session()).await)?;
        standing(bounded(Err(Fault::Lost), self.system.availability()).await)
    }
}

impl PresencePrompt for WindowsHelloPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async move {
            match self.ready().await {
                Ok(()) => Availability::ready(
                    PromptName::WindowsHello,
                    "Approvals on this machine ask for Windows Hello: your face, a fingerprint, or your PIN. The prompt names the tool and the first twelve characters of the change, as the page does.",
                ),
                Err(why) => why,
            }
        })
    }

    fn ask(&self, text: PromptText, deadline: Instant) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            if let Err(why) = self.ready().await {
                return PromptAnswer::NotAvailable(why);
            }
            // At or after the deadline nothing is raised.
            if Instant::now() >= deadline {
                return PromptAnswer::Dismissed;
            }
            tokio::select! {
                // The deadline is looked at first: at or after it the
                // answer is a dismissal, whatever Windows Hello says. The
                // verification's future is dropped, which cancels it.
                biased;
                () = tokio::time::sleep_until(deadline) => PromptAnswer::Dismissed,
                answer = self.system.verify(message_for(&text)) => answered(answer),
            }
        })
    }
}

/// The real Windows Hello and session of this machine.
#[cfg(windows)]
#[allow(unsafe_code)] // calls into Windows; each is explained where it is made
mod system {
    use std::ffi::c_void;
    use std::ptr;

    use a2m_actions::BoxFuture;
    use tokio::sync::oneshot;
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::{HSTRING, factory};
    use windows_future::IAsyncOperation;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::RemoteDesktop::{
        ProcessIdToSessionId, WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION,
        WTS_SESSIONSTATE_LOCK, WTS_SESSIONSTATE_UNLOCK, WTSFreeMemory, WTSINFOEXW,
        WTSQuerySessionInformationW, WTSSessionInfoEx,
    };
    use windows_sys::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_NAME,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, GetSystemMetrics, MSG,
        PostThreadMessageW, SM_REMOTESESSION, TranslateMessage, WM_QUIT, WS_EX_TOOLWINDOW,
        WS_POPUP,
    };

    use super::{Fault, Lock, Session, Verifier};

    /// The interactive window station's name.
    const INTERACTIVE_STATION: &str = "WinSta0";
    /// Room for a window station's name, in UTF-16 units.
    const NAME_ROOM: usize = 64;

    fn last_error() -> Fault {
        // SAFETY: no condition.
        Fault::Call(unsafe { GetLastError() }.cast_signed())
    }

    fn call(error: &windows::core::Error) -> Fault {
        Fault::Call(error.code().0)
    }

    /// `text`, as UTF-16 with its terminating nul.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn station_is_interactive() -> Result<bool, Fault> {
        // SAFETY: no condition. The handle is the process's own and is not
        // to be closed.
        let station = unsafe { GetProcessWindowStation() };
        if station.is_null() {
            return Err(last_error());
        }
        let mut name = [0_u16; NAME_ROOM];
        let mut needed = 0_u32;
        // SAFETY: the buffer is `size_of_val(&name)` bytes long, as the
        // length says, and `needed` is a place for a `u32`.
        let ok = unsafe {
            GetUserObjectInformationW(
                station,
                UOI_NAME,
                name.as_mut_ptr().cast::<c_void>(),
                u32::try_from(size_of_val(&name)).unwrap_or(0),
                &raw mut needed,
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        let end = name.iter().position(|&unit| unit == 0).unwrap_or(NAME_ROOM);
        Ok(String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case(INTERACTIVE_STATION))
    }

    fn lock_state() -> Result<Lock, Fault> {
        let mut buffer: *mut u16 = ptr::null_mut();
        let mut bytes = 0_u32;
        // SAFETY: both are places of the right type. On success the system
        // allocates the buffer, which is freed below, once.
        let ok = unsafe {
            WTSQuerySessionInformationW(
                WTS_CURRENT_SERVER_HANDLE,
                WTS_CURRENT_SESSION,
                WTSSessionInfoEx,
                &raw mut buffer,
                &raw mut bytes,
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        let whole = usize::try_from(bytes).is_ok_and(|bytes| bytes >= size_of::<WTSINFOEXW>());
        let flags = if buffer.is_null() || !whole {
            None
        } else {
            // SAFETY: the buffer holds one `WTSINFOEXW`, as the length just
            // checked says, allocated by the system with the alignment of
            // any structure. Level 1 is checked before the union is read.
            let info = unsafe { &*buffer.cast::<WTSINFOEXW>() };
            if info.Level == 1 {
                // SAFETY: level 1 says the union holds its level-1 member.
                Some(unsafe { info.Data.WTSInfoExLevel1.SessionFlags })
            } else {
                None
            }
        };
        // SAFETY: the buffer came from `WTSQuerySessionInformationW` and is
        // freed once; nothing read from it is used afterwards.
        unsafe { WTSFreeMemory(buffer.cast::<c_void>()) };
        Ok(match flags.map(i32::cast_unsigned) {
            Some(WTS_SESSIONSTATE_LOCK) => Lock::Locked,
            Some(WTS_SESSIONSTATE_UNLOCK) => Lock::Unlocked,
            _ => Lock::Unknown,
        })
    }

    fn read_session() -> Result<Session, Fault> {
        let mut id = 0_u32;
        // SAFETY: `id` is a place for a `u32`; the process id is this one's.
        if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut id) } == 0 {
            return Err(last_error());
        }
        let interactive_station = station_is_interactive()?;
        // SAFETY: no condition.
        let remote = unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0;
        Ok(Session {
            services: id == 0,
            interactive_station,
            remote,
            lock: lock_state()?,
        })
    }

    /// The window that owns the dialog, and the thread that runs its
    /// messages. Dropping it ends the thread, which closes the window.
    struct Owner {
        /// The window's handle, as an address: a handle may not move
        /// between threads, its number may.
        window: usize,
        thread: u32,
    }

    impl Owner {
        async fn open() -> Result<Self, Fault> {
            let (sender, receiver) = oneshot::channel();
            std::thread::Builder::new()
                .name("a2m-presence-owner".to_owned())
                .spawn(move || run_owner(sender))
                .map_err(|_| Fault::NoWindow)?;
            receiver.await.unwrap_or(Err(Fault::NoWindow))
        }

        fn handle(&self) -> HWND {
            HWND(ptr::without_provenance_mut::<c_void>(self.window))
        }
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            // SAFETY: no condition. A thread that has already ended makes
            // the call fail, and then there is nothing to close.
            unsafe { PostThreadMessageW(self.thread, WM_QUIT, 0, 0) };
        }
    }

    /// Makes the owner window, sends it back, and runs the thread's
    /// messages until it is told to quit; then destroys the window.
    fn run_owner(sender: oneshot::Sender<Result<Owner, Fault>>) {
        let class = wide("STATIC");
        let title = wide("Anything2MCP approval");
        // SAFETY: both strings end with a nul and outlive the call. A class
        // the system predefines needs no instance; there is no parent, no
        // menu, and no parameter. The window is never shown.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if window.is_null() {
            let _ = sender.send(Err(Fault::NoWindow));
            return;
        }
        // The thread has a message queue now, since it has a window, so a
        // quit posted to it from here on arrives. If nobody waits for the
        // owner any more, the owner is dropped here and posts it at once.
        let owner = Owner {
            window: window.addr(),
            // SAFETY: no condition.
            thread: unsafe { GetCurrentThreadId() },
        };
        let _ = sender.send(Ok(owner));
        let mut message = MSG::default();
        // SAFETY: `message` is a place for one message; a null window takes
        // every message of this thread. The loop ends on a quit (0) or an
        // error (-1).
        while unsafe { GetMessageW(&raw mut message, ptr::null_mut(), 0, 0) } > 0 {
            // SAFETY: the message is the one just taken.
            unsafe {
                TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        // SAFETY: the window was made by this thread, the only one that may
        // destroy it, and is destroyed once.
        unsafe { DestroyWindow(window) };
    }

    /// Cancels the operation when the ask ends or is dropped. Cancelling an
    /// operation that has ended has no effect.
    struct Cancel(IAsyncOperation<UserConsentVerificationResult>);

    impl Drop for Cancel {
        fn drop(&mut self) {
            let _ = self.0.Cancel();
        }
    }

    /// Windows Hello on this machine.
    pub(super) struct Hello;

    impl Verifier for Hello {
        fn session(&self) -> BoxFuture<'_, Result<Session, Fault>> {
            Box::pin(async {
                tokio::task::spawn_blocking(read_session)
                    .await
                    .unwrap_or(Err(Fault::Lost))
            })
        }

        fn availability(&self) -> BoxFuture<'_, Result<i32, Fault>> {
            Box::pin(async {
                let operation =
                    UserConsentVerifier::CheckAvailabilityAsync().map_err(|error| call(&error))?;
                let availability = operation.await.map_err(|error| call(&error))?;
                Ok(availability.0)
            })
        }

        fn verify(&self, message: String) -> BoxFuture<'_, Result<i32, Fault>> {
            Box::pin(async move {
                // Dropped last: the operation is cancelled before its
                // window is closed.
                let owner = Owner::open().await?;
                let operation: IAsyncOperation<UserConsentVerificationResult> = {
                    // The interop is not kept across an await: it may not
                    // move between threads.
                    let interop = factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
                        .map_err(|error| call(&error))?;
                    // SAFETY: the handle is a window of this process that
                    // stays open until `owner` is dropped, after the
                    // operation has ended or been cancelled. The message is
                    // a string the call copies.
                    unsafe {
                        interop.RequestVerificationForWindowAsync(
                            owner.handle(),
                            &HSTRING::from(message.as_str()),
                        )
                    }
                    .map_err(|error| call(&error))?
                };
                let cancel = Cancel(operation.clone());
                let result = operation.await.map_err(|error| call(&error));
                drop(cancel);
                drop(owner);
                Ok(result?.0)
            })
        }
    }
}
