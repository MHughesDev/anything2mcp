//! Test stand: a command-line program with typed arguments, several output
//! forms, a prompt, and slow output.
//!
//! The program is `example-cli`, an inventory tool for the imaginary
//! order-entry system named `example`. It exists so that the command pack can
//! be proven against something that behaves like a business command-line tool
//! and can be made to misbehave on demand. The cases it imitates are those of
//! the test stand in spec section 306, and the `README.md` beside this crate
//! lists them.
//!
//! A test starts a stand with one call, [`harness::Stand::start`], which gives
//! it a program of its own, a fresh home folder, and a control interface. The
//! control interface reports ground truth without going through the program's
//! own read commands: what the data file holds, how many times each item was
//! written and by which start, and every start the program received.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering. A product crate may use it in tests only.

mod control;
pub mod faults;
pub mod harness;
pub mod program;
pub mod store;

pub use control::{CONTROL_FOLDER, Invocation};

/// The version the program reports.
pub const VERSION: &str = "2.4.1";

/// The variable that names the home folder, when `--home` does not.
pub const HOME_ENV: &str = "EXAMPLE_CLI_HOME";
/// The variable that names the control folder, when there is none beside the program.
pub const CONTROL_ENV: &str = "EXAMPLE_CLI_CONTROL";
/// The variable through which the program accepts its password instead of a
/// kept session.
pub const TOKEN_ENV: &str = "EXAMPLE_CLI_TOKEN";

/// The one password `login` accepts. It is synthetic and guards nothing.
pub const PASSWORD: &str = "example-stand-password";

/// What the program asks, on standard output, before a write without `--yes`.
pub const CONFIRM_PROMPT: &str = "Are you sure? [y/N] ";
/// What `login` asks, on standard error, unless given `--password-stdin`.
pub const PASSWORD_PROMPT: &str = "Password: ";

/// What each exit status of the program means.
pub mod exit {
    /// The command did its work.
    pub const OK: u8 = 0;
    /// The program itself failed: a file could not be read or written.
    pub const INTERNAL: u8 = 1;
    /// The arguments were wrong, or a question was not answered with yes.
    /// Nothing changed.
    pub const INPUT: u8 = 2;
    /// There is no such item. Nothing changed.
    pub const ABSENT: u8 = 3;
    /// A business rule refused the request. Nothing changed.
    pub const REJECTED: u8 = 4;
    /// The item no longer has the version the caller expected. Nothing changed.
    pub const STALE: u8 = 5;
    /// Another start of the program is writing. Nothing changed.
    pub const BUSY: u8 = 6;
    /// A hung program gave up waiting to be killed.
    pub const HANG_ENDED: u8 = 70;
    /// Try again later. The program never exits with it by itself; a fault does.
    pub const TRANSIENT: u8 = 75;
    /// The program is not signed in and has to be. Nothing changed.
    pub const SESSION_EXPIRED: u8 = 77;
}

/// The stand builds its program twice. The two builds behave the same and
/// report the same version, and their files differ, so a test can show a
/// program replaced by another build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    /// The build a stand starts with: the binary `example-cli`.
    First,
    /// The other build: the binary `example-cli-rebuilt`.
    Rebuilt,
}

impl Build {
    /// The name the invocation log gives this build.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Rebuilt => "rebuilt",
        }
    }

    /// The name of this build's binary, without the system's suffix.
    #[must_use]
    pub fn binary(self) -> &'static str {
        match self {
            Self::First => "example-cli",
            Self::Rebuilt => "example-cli-rebuilt",
        }
    }
}
