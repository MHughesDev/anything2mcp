//! What the base reports when something cannot be done, and the few
//! conversions that mean the same whatever the step was.

use std::fmt;

use a2m_core::{Failure, FailureClass, FailureCode};

/// The failure codes this crate produces. Each belongs to exactly one class.
pub mod codes {
    use a2m_core::{FailureClass, FailureCode};

    /// No browser program was found.
    pub const BROWSER_UNAVAILABLE: FailureCode = FailureCode::fixed("browser_unavailable");
    /// The program that started is not a browser this crate can own.
    pub const BROWSER_NOT_OWNED: FailureCode = FailureCode::fixed("browser_not_owned");
    /// The link to the browser broke before the request was written.
    pub const CONNECTION_LOST: FailureCode = FailureCode::fixed("connection_lost");
    /// A deadline passed before the request was written, or on a read.
    pub const TIMEOUT: FailureCode = FailureCode::fixed("timeout");
    /// A password input was asked for its value.
    pub const PASSWORD_READ_REFUSED: FailureCode = FailureCode::fixed("password_read_refused");
    /// An address is not one a tab may be sent to.
    pub const ADDRESS_INVALID: FailureCode = FailureCode::fixed("address_invalid");
    /// A value cannot stand in one segment of a path: it is refused, never
    /// encoded.
    pub const ADDRESS_VALUE_REFUSED: FailureCode = FailureCode::fixed("address_value_refused");
    /// A page, or an address, is on an origin the tab was not opened for.
    /// The same name, with the same class, as the code the packs built on
    /// this crate give a surface nobody declared.
    pub const SURFACE_NOT_DECLARED: FailureCode = FailureCode::fixed("surface_not_declared");

    /// Every code of this crate with its one class.
    pub const ALL: &[(FailureClass, &str)] = &[
        (FailureClass::Busy, "browser_unavailable"),
        (FailureClass::UnknownVariant, "browser_not_owned"),
        (FailureClass::Transient, "connection_lost"),
        (FailureClass::Transient, "timeout"),
        (FailureClass::Policy, "password_read_refused"),
        (FailureClass::Input, "address_invalid"),
        (FailureClass::Input, "address_value_refused"),
        (FailureClass::Policy, "surface_not_declared"),
    ];
}

/// Whether a request reached the page. The crate above decides what that
/// means for a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dispatch {
    /// The whole request was written to the browser.
    Sent,
    /// Nothing of the request was written.
    NotSent,
    /// Writing began and did not finish, or it cannot be told.
    Unknown,
}

/// Why an element cannot be acted on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Reason {
    /// It is not laid out, or is hidden.
    NotVisible,
    /// It is disabled.
    Disabled,
    /// It cannot be edited: it is read only, or is no input at all.
    NotEditable,
    /// Another element would receive the event.
    Obscured,
    /// It is not a control this input fits, such as a check on a text box.
    WrongControl,
    /// The option asked for is not one the control offers.
    NoSuchOption,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotVisible => "it is not visible",
            Self::Disabled => "it is disabled",
            Self::NotEditable => "it cannot be edited",
            Self::Obscured => "another element would receive the event",
            Self::WrongControl => "it is not a control this input fits",
            Self::NoSuchOption => "it does not offer the option asked for",
        })
    }
}

/// Why no browser was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchReason {
    /// No browser program was found in settings or in the known places.
    NotInstalled,
    /// A window was asked for, and the engine's environment names no screen
    /// to show it on (decision 0254). Nothing was started. The crate above
    /// classes it.
    NoScreen,
    /// The supervisor did not start the program; this is its failure, kept
    /// whole: an undeclared program, a limit, a place that is taken.
    Supervisor(Failure),
    /// The program started and is not a browser this crate can own: its
    /// pipe ended or carried something else, it answered with another
    /// protocol, or its digest is not a known one. It was closed.
    NotOwned(String),
    /// The program started, its pipe stayed open, and it did not answer
    /// before the launch deadline. It was closed. It may be a browser that
    /// starts slowly, so this passes: nothing reached any system.
    NoAnswer(String),
    /// The data directory could not be made inside the profile root.
    DataDirectory(Failure),
}

/// The shape of a request an observing tab refused: its method and where it
/// was going, without the query.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestShape {
    /// The method of a request; or what was refused when the page made
    /// it, such as `WEBSOCKET`; or `WORKER` for a worker that was not let
    /// run.
    pub method: String,
    /// The address, cut at the query, with every known secret replaced.
    pub address: String,
}

/// What the base reports. The crates above map it, knowing what the step
/// was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserError {
    /// The query matched nothing, or the element is of an earlier page
    /// generation.
    NotFound,
    /// The query matched more than one element. The first is never picked.
    Ambiguous {
        /// How many matched.
        count: usize,
    },
    /// The element was found and cannot take the input. Nothing was sent.
    NotActionable(Reason),
    /// The deadline passed.
    Timeout {
        /// Whether the request had reached the browser.
        dispatch: Dispatch,
    },
    /// The page crashed.
    Crashed {
        /// Whether the request had reached the browser.
        dispatch: Dispatch,
    },
    /// The link to the browser ended, or the tab was closed.
    Disconnected {
        /// Whether the request had reached the browser.
        dispatch: Dispatch,
    },
    /// A dialog of the page is open and nothing else can be done until it
    /// is answered.
    UnexpectedDialog {
        /// Whether the request had reached the browser.
        dispatch: Dispatch,
    },
    /// The page did not load: the browser's own reason, such as a refused
    /// connection.
    NotLoaded(String),
    /// A password input was asked for its value, or a selector asked about
    /// the `value` attribute on a page that holds a password in it. It is
    /// never read, and never told match by match.
    PasswordField,
    /// The page is on an origin the tab was not opened for, or the tab was
    /// asked to go, or to send a request, to one. Nothing was located,
    /// read, or sent there. It holds that origin, or, for a page that has
    /// none, its scheme.
    ForeignOrigin(String),
    /// What was read is larger than the limit. Nothing is cut silently.
    TooLarge {
        /// The limit, in characters.
        limit: usize,
    },
    /// A request sent from inside the page did not complete. Whether it was
    /// sent is not known.
    RequestFailed(String),
    /// A request sent from inside the page was answered with a redirect,
    /// which it does not follow: the application answered, and where it
    /// sent the request is not looked at (decision 0195).
    Redirected,
    /// The browser, or the helper in the page, answered in a shape no rule
    /// covers.
    Unexpected(String),
    /// A write from inside the page was not sent: the page's anti-forgery
    /// token is not where the entry says, or is not one that can be sent
    /// (decision 0206).
    Token(crate::page_write::TokenProblem),
    /// A write from inside the page was refused by the helper before
    /// anything left: the page has no origin of its own, or the write was
    /// not one the helper sends.
    WriteRefused(String),
    /// No browser was opened.
    LaunchFailed(LaunchReason),
}

impl BrowserError {
    /// Whether the request this error belongs to reached the browser. Every
    /// error answers: the ones that stop before anything is written say
    /// `NotSent`.
    #[must_use]
    pub fn dispatch(&self) -> Dispatch {
        match self {
            Self::Timeout { dispatch }
            | Self::Crashed { dispatch }
            | Self::Disconnected { dispatch }
            | Self::UnexpectedDialog { dispatch } => *dispatch,
            Self::RequestFailed(_) => Dispatch::Unknown,
            // The browser answered, so the request reached it.
            Self::Unexpected(_) | Self::TooLarge { .. } | Self::Redirected => Dispatch::Sent,
            Self::NotFound
            | Self::Ambiguous { .. }
            | Self::NotActionable(_)
            | Self::NotLoaded(_)
            | Self::PasswordField
            | Self::ForeignOrigin(_)
            | Self::Token(_)
            | Self::WriteRefused(_)
            | Self::LaunchFailed(_) => Dispatch::NotSent,
        }
    }

    /// The failure this error is whatever the step was, when it has one.
    /// `None` for the errors only the crate above can class: a query that
    /// found nothing is `drift` for a screen model and an answer for a
    /// probe, and a lost link after a request was sent is `possible_commit`
    /// for a commit point and `transient` for a read.
    #[must_use]
    pub fn failure(&self) -> Option<Failure> {
        let made = |class, code: FailureCode, message: &str, next: &str| {
            Some(Failure::new(class, code, message, next))
        };
        match self {
            Self::LaunchFailed(LaunchReason::NotInstalled) => made(
                FailureClass::Busy,
                codes::BROWSER_UNAVAILABLE,
                "No browser was found: none is named in the settings, and none is installed in a known place.",
                "Install Chrome, Edge, or Chromium, or name the browser program in the settings.",
            ),
            Self::LaunchFailed(
                LaunchReason::Supervisor(failure) | LaunchReason::DataDirectory(failure),
            ) => Some(failure.clone()),
            Self::LaunchFailed(LaunchReason::NotOwned(why)) => Some(Failure::new(
                FailureClass::UnknownVariant,
                codes::BROWSER_NOT_OWNED,
                format!("The program that was started is not a browser the engine can own: {why}."),
                "Check the browser program named in the settings, and its version.",
            )),
            Self::LaunchFailed(LaunchReason::NoAnswer(why)) => Some(Failure::new(
                FailureClass::Transient,
                codes::TIMEOUT,
                format!("The browser was started and closed again: {why}."),
                "Try again; a browser's first start on a cold machine can be slow. If it persists, check the browser program, or allow more time in the settings.",
            )),
            Self::Crashed {
                dispatch: Dispatch::NotSent,
            }
            | Self::Disconnected {
                dispatch: Dispatch::NotSent,
            } => made(
                FailureClass::Transient,
                codes::CONNECTION_LOST,
                "The browser was lost before the request was sent.",
                "Try again; the browser is started afresh.",
            ),
            Self::Timeout {
                dispatch: Dispatch::NotSent,
            } => made(
                FailureClass::Transient,
                codes::TIMEOUT,
                "The deadline passed before the request was sent to the browser.",
                "Try again, or allow more time.",
            ),
            Self::PasswordField => made(
                FailureClass::Policy,
                codes::PASSWORD_READ_REFUSED,
                "The value of a password input is never read.",
                "Remove the read of this field from the target's files.",
            ),
            Self::WriteRefused(why) => Some(Failure::new(
                FailureClass::Input,
                codes::ADDRESS_INVALID,
                format!("A write from inside the page was not sent: {why}."),
                "Look at the page the write is made from, and at the entry's path.",
            )),
            Self::ForeignOrigin(origin) => Some(Failure::new(
                FailureClass::Policy,
                codes::SURFACE_NOT_DECLARED,
                format!(
                    "The page is on `{origin}`, or was to be sent there, and that is not an origin this target declares. Nothing was read or sent there."
                ),
                "Declare the surface in the target's files if the system really lives there; otherwise look at why the page moved.",
            )),
            _ => None,
        }
    }
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sent = |dispatch: &Dispatch| match dispatch {
            Dispatch::Sent => "the request was sent",
            Dispatch::NotSent => "the request was not sent",
            Dispatch::Unknown => "whether the request was sent is not known",
        };
        match self {
            Self::NotFound => {
                f.write_str("no element matches, or the element is of an earlier page")
            }
            Self::Ambiguous { count } => write!(f, "{count} elements match, and exactly one must"),
            Self::NotActionable(reason) => write!(f, "the element cannot take the input: {reason}"),
            Self::Timeout { dispatch } => write!(f, "the deadline passed; {}", sent(dispatch)),
            Self::Crashed { dispatch } => write!(f, "the page crashed; {}", sent(dispatch)),
            Self::Disconnected { dispatch } => {
                write!(f, "the browser was lost; {}", sent(dispatch))
            }
            Self::UnexpectedDialog { dispatch } => {
                write!(f, "a dialog of the page is open; {}", sent(dispatch))
            }
            Self::NotLoaded(why) => write!(f, "the page did not load: {why}"),
            Self::PasswordField => f.write_str("the value of a password input is never read"),
            Self::ForeignOrigin(origin) => write!(
                f,
                "`{origin}` is not an origin the tab was opened for; nothing was read or sent there"
            ),
            Self::TooLarge { limit } => {
                write!(
                    f,
                    "what was read is longer than the limit of {limit} characters"
                )
            }
            Self::RequestFailed(why) => {
                write!(f, "a request from the page did not complete: {why}")
            }
            Self::Redirected => f.write_str(
                "a request from the page was answered with a redirect, which it does not follow",
            ),
            Self::Unexpected(what) => write!(f, "an answer no rule covers: {what}"),
            Self::Token(problem) => write!(f, "{problem}; nothing was sent"),
            Self::WriteRefused(why) => {
                write!(f, "the write was not sent from the page: {why}")
            }
            Self::LaunchFailed(reason) => match reason {
                LaunchReason::NotInstalled => f.write_str("no browser is installed"),
                LaunchReason::NoScreen => {
                    f.write_str("a window was asked for, and there is no screen to show it on")
                }
                LaunchReason::Supervisor(failure) | LaunchReason::DataDirectory(failure) => {
                    write!(f, "the browser was not started: {failure}")
                }
                LaunchReason::NoAnswer(why) => {
                    write!(f, "the browser was started and closed again: {why}")
                }
                LaunchReason::NotOwned(why) => {
                    write!(
                        f,
                        "the program started is not a browser the engine can own: {why}"
                    )
                }
            },
        }
    }
}

impl std::error::Error for BrowserError {}

pub(crate) fn input(code: FailureCode, message: impl Into<String>, next: &str) -> Failure {
    Failure::new(FailureClass::Input, code, message, next)
}
