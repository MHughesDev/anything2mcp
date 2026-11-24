//! The failure codes the MCP pack produces. Each belongs to exactly one
//! class, and each is declared in the pack's manifest; a test holds the two
//! lists together. The codes of other crates that the driver returns as
//! they are, and the manifest declares too, are in [`PASSED_ON`].
//!
//! No sentence here quotes anything an upstream server said: its text is
//! untrusted and never becomes part of a failure (spec section 502).
//!
//! The pack has no `busy` code.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::StepFailure;

a2m_core::failure_codes! {
    UPSTREAM_TOOL_CHANGED = Drift, "upstream_tool_changed",
        meaning "A mapped tool's definition is not the one its mapping pins, or the tool is gone.",
        next "Run discovery on the surface, review what changed, and pin the tool's new digest in the mapping.";
    RESPONSE_SCHEMA_MISMATCH = Drift, "response_schema_mismatch",
        meaning "A result does not fit the pinned output schema.",
        next "Read the record back before anything is written again; then run discovery and compare the tool's definition with its mapping.";
    APP_VALIDATION_ERROR = Rejected, "app_validation_error",
        meaning "The tool answered a well-formed call with its own error result.",
        next "Look at what the tool said, which is shown apart as untrusted text, and correct the input.";
    UNEXPECTED_STATUS = UnknownVariant, "unexpected_status",
        meaning "The server answered with something the protocol or the mapping did not foresee: an unforeseen protocol error, a status, a body that is not a response, a revision this build does not speak.",
        next "Check the server and its mapping, and run discovery on the surface.";
    AMBIGUOUS_MATCH = PossibleCommit, "ambiguous_match",
        meaning "A lookup found more than one record.",
        next "Look at the records in the system and decide which one the run made, before anything is written again.";
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable",
        meaning "The server could not be reached, or said it cannot serve now.",
        next "Check that the upstream server runs and can be reached, then try again.";
    NOT_GRANTED = Policy, "not_granted",
        meaning "A call to a tool the mapping does not allow.",
        next "Map the tool in the surface's mapping file, review it, and publish the target again.";
    LINK_NOT_ENCRYPTED = Policy, "link_not_encrypted",
        meaning "A credential would cross plain `http` to an address that is not a loopback address, from a surface that does not accept it. Nothing is sent (decision 0188).",
        next "Reach the server over `https`. For a server on a private network that offers nothing else, the surface may declare `unencrypted_credentials: accepted` in `target.yaml`.";
    PROGRAM_PATH_UNSAFE = Policy, "program_path_unsafe",
        meaning "A local server program, a launcher, or a server's entry file that another user could change, or a program that is a shell, an interpreter, or a script, is not started (decision 0250).",
        next "Name a program that is itself the server, or a launcher of the provider's table with the server's entry file, by absolute paths on a local drive, and give each file and every folder above it to the engine's user or the administrator, with no write access for anyone else.";
    ENTRY_DIGEST_CHANGED = Drift, "entry_digest_changed",
        meaning "The entry file of a server started through a launcher is not the one its `entry_digest` pins. Nothing is started (decision 0399).",
        next "Review the installed server's entry file, write the SHA-256 of its bytes under the surface's `entry_digest`, and publish.";
    PACKAGE_RUNNER_REFUSED = Policy, "package_runner_refused",
        meaning "The local server program is a package runner, which fetches the code it runs at the moment it starts; nothing the product starts fetches code then (decision 0299). Nothing is started.",
        next "Install the server's package, then name the launcher of the provider's table as the surface's `program` and the installed entry file as `entry`, each with its digest.";
    LAUNCHER_NOT_IN_TABLE = Policy, "launcher_not_in_table",
        meaning "A surface names an entry file, and its program is not a launcher of the provider's closed table of interpreter forms, or is not given that launcher's one form. Nothing is started (decision 0399).",
        next "Name a launcher of the table as `program`, with the fixed arguments of its form as `launcher_args`, or name a program that is itself the server and no entry file.";
    PRODUCT_HEADER_REFUSED = Policy, "product_header_refused",
        meaning "A session's provider placed a header whose name begins `a2m-`, one of the product's own carriers between its parts, which never goes on the wire. Nothing is sent (decision 0270).",
        next "Sign in to this surface with `upstream_mcp`, or with a provider made for an HTTP service, such as `bearer_static` or `api_key`.";
    ARGUMENTS_INVALID = Input, "arguments_invalid",
        meaning "The arguments of a call do not fit the tool's pinned input schema.",
        next "Correct the binding's `args`, or the input, so that they fit the tool's input schema.";
    TLS_NOT_ENABLED = Input, "tls_not_enabled",
        meaning "The server is reached over HTTPS, and this build cannot make HTTPS requests.",
        next "Reach the server over plain HTTP on a loopback address, or use a build with TLS.";
    NOT_BUILT = Input, "not_built",
        meaning "A part of the pack's design that this build does not carry out.",
        next "See the build order in the design of the crate `a2m-pack-mcp` for what is built.";
}

/// From the provider contract: the answer did not come in time.
pub const TIMEOUT: FailureCode = a2m_provider::codes::TIMEOUT;
/// From the provider contract: the connection, or the server program, was
/// lost.
pub const CONNECTION_LOST: FailureCode = a2m_provider::codes::CONNECTION_LOST;

/// The codes of other crates the driver returns as they are, which the
/// manifest declares too: the provider contract's `timeout` and
/// `connection_lost`, the core's `payload_unreadable` for a published plan
/// that does not hold what the pack compiled into it, and the session
/// keeper's TLS failures (decision 0157), every code of `a2m_auth::tls` but
/// `trust_roots_unreadable`, which only the engine's start gives.
pub const PASSED_ON: &[FailureCode] = &[
    TIMEOUT,
    CONNECTION_LOST,
    FailureCode::PAYLOAD_UNREADABLE,
    a2m_auth::tls::TLS_SETUP_FAILED,
    a2m_auth::tls::CERTIFICATE_UNTRUSTED,
    a2m_auth::tls::CERTIFICATE_NAME_MISMATCH,
    a2m_auth::tls::CERTIFICATE_EXPIRED,
    a2m_auth::tls::CERTIFICATE_NOT_VERIFIED,
    a2m_auth::tls::TLS_HANDSHAKE_FAILED,
];

/// A credential would cross an unencrypted link to another machine.
pub(crate) fn link_not_encrypted() -> Failure {
    Failure::new(
        FailureClass::Policy,
        LINK_NOT_ENCRYPTED,
        "The upstream server is reached over plain `http` at an address that is not a loopback address, and the connection would carry the session's credential over that unencrypted link; nothing was sent.",
        "Reach the server over `https`. For a server on a private network that offers nothing else, the surface may declare `unencrypted_credentials: accepted` in `target.yaml`.",
    )
}

const RE_PIN: &str = "Run discovery on the surface, review what changed, and pin the tool's new digest in the mapping.";

pub(crate) fn tool_changed(tool: &str, what: &str) -> Failure {
    Failure::new(
        FailureClass::Drift,
        UPSTREAM_TOOL_CHANGED,
        format!("The upstream tool `{}` {what}.", shown(tool)),
        RE_PIN,
    )
}

pub(crate) fn schema_mismatch(tool: &str) -> Failure {
    Failure::new(
        FailureClass::Drift,
        RESPONSE_SCHEMA_MISMATCH,
        format!(
            "The result of the upstream tool `{}` does not fit its pinned output schema.",
            shown(tool)
        ),
        "Read the record back before anything is written again; then run discovery and compare the tool's definition with its mapping.",
    )
}

pub(crate) fn rejected(tool: &str) -> Failure {
    Failure::new(
        FailureClass::Rejected,
        APP_VALIDATION_ERROR,
        format!(
            "The upstream tool `{}` refused the call with its own error result.",
            shown(tool)
        ),
        "Look at what the tool said, which is shown apart as untrusted text, and correct the input.",
    )
}

pub(crate) fn unexpected(what: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        UNEXPECTED_STATUS,
        what,
        "Nothing was guessed. Check the server and its mapping, and run discovery on the surface.",
    )
}

pub(crate) fn ambiguous(tool: &str, count: usize) -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        AMBIGUOUS_MATCH,
        format!(
            "The lookup through the upstream tool `{}` found {count} records where one was expected.",
            shown(tool)
        ),
        "Look at the records in the system and decide which one the run made, before anything is written again.",
    )
}

pub(crate) fn timeout(what: &str) -> Failure {
    Failure::new(
        FailureClass::Transient,
        TIMEOUT,
        format!("The upstream server did not answer {what} in time."),
        "Try again later; if the call was a write, read the record back first.",
    )
}

pub(crate) fn connection_lost(what: &str) -> Failure {
    Failure::new(
        FailureClass::Transient,
        CONNECTION_LOST,
        format!("The connection to the upstream server was lost {what}."),
        "Try again; if a write was sent, read the record back before anything is written again.",
    )
}

pub(crate) fn unavailable(what: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::Transient,
        UPSTREAM_UNAVAILABLE,
        what,
        "Check that the upstream server runs and can be reached, then try again.",
    )
}

pub(crate) fn not_granted(what: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::Policy,
        NOT_GRANTED,
        what,
        "Map the tool in the surface's mapping file, review it, and publish the target again.",
    )
}

pub(crate) fn arguments_invalid(tool: &str, why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        ARGUMENTS_INVALID,
        format!(
            "The arguments for the upstream tool `{}` do not fit its pinned input schema: {why}.",
            shown(tool)
        ),
        "Correct the binding's `args`, or the input, so that they fit the tool's input schema.",
    )
}

#[cfg(not(feature = "tls"))]
pub(crate) fn tls_not_enabled() -> Failure {
    Failure::new(
        FailureClass::Input,
        TLS_NOT_ENABLED,
        "The upstream server is reached over HTTPS, and TLS is not built into this build.",
        "Reach the server over plain HTTP on a loopback address, or use a build with TLS.",
    )
}

/// A header of the product's own, placed by a session's provider. Its name
/// is the product's and is said; no value is.
pub(crate) fn product_header_refused(name: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        PRODUCT_HEADER_REFUSED,
        format!(
            "The session's sign-in placed the header `{}`, which belongs to the product and never goes to an upstream server; nothing was sent.",
            shown(name)
        ),
        "Sign in to this surface with `upstream_mcp`, or with a provider made for an HTTP service, such as `bearer_static` or `api_key`.",
    )
}

pub(crate) fn not_built(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        NOT_BUILT,
        format!("This build of the MCP pack cannot {what}."),
        "See the build order in the design of the crate `a2m-pack-mcp` for what is built.",
    )
}

pub(crate) fn plan_unreadable(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::PAYLOAD_UNREADABLE,
        format!("The published plan does not hold what the MCP pack compiled into it: {what}."),
        "Compile the target again with the versions of the packs that are enabled.",
    )
}

/// The refusal of a variable that may not be handed to a local server
/// program, found in a plan that holds one all the same.
pub(crate) fn variable_refused(name: &str, why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::PAYLOAD_UNREADABLE,
        format!(
            "The published plan hands a local server program the variable `{}`, which {why}.",
            shown(name)
        ),
        "Remove the variable from the surface and compile the target again. A credential for a local server program goes through the session keeper, never through its environment.",
    )
}

/// The entry file of a server started through a launcher is not the one its
/// digest pins; `place` says where it was read.
pub(crate) fn entry_digest_changed(path: &str, place: &str) -> Failure {
    Failure::new(
        FailureClass::Drift,
        ENTRY_DIGEST_CHANGED,
        format!(
            "The entry file `{}` of the local server {place} is not the one its `entry_digest` names; nothing was started.",
            shown_path(path)
        ),
        "Review the installed server's entry file, write the SHA-256 of its bytes under the surface's `entry_digest`, and publish.",
    )
}

/// The local server program is a package runner.
pub(crate) fn package_runner_refused(path: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        PACKAGE_RUNNER_REFUSED,
        format!(
            "The local server program `{}` is a package runner: it fetches the code it runs when it starts, and nothing the product starts fetches code then; nothing was started.",
            shown_path(path)
        ),
        "Install the server's package, then name the launcher of the provider's table as the surface's `program` and the installed entry file as `entry`, each with its digest.",
    )
}

/// The program of a surface that names an entry file is not a launcher of
/// the table, or not in its form; `why` says which.
pub(crate) fn launcher_not_in_table(path: &str, why: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        LAUNCHER_NOT_IN_TABLE,
        format!(
            "The local server program `{}` is given an entry file, and {why}; nothing was started.",
            shown_path(path)
        ),
        "Name a launcher of the table as `program`, with the fixed arguments of its form as `launcher_args`, or name a program that is itself the server and no entry file.",
    )
}

/// Why the provider's check of who may change a program refused the file
/// at `path`, named by `noun`: the local server program, the launcher, or
/// the entry file of a server started through a launcher. A file that is
/// not there is the contract's `input`/`program_not_found`, as the
/// supervisor says it; every other refusal is `policy`/`program_path_unsafe`.
pub(crate) fn file_refused(why: &a2m_provider::ProgramRefused, path: &str, noun: &str) -> Failure {
    use a2m_provider::ProgramRefused;
    let at = shown_path(path);
    let unsafe_path = |what: String, next: &str| {
        Failure::new(
            FailureClass::Policy,
            PROGRAM_PATH_UNSAFE,
            what,
            next.to_owned(),
        )
    };
    let place = |path: &std::path::Path| shown_path(&path.display().to_string());
    match why {
        ProgramRefused::Missing => Failure::new(
            FailureClass::Input,
            a2m_provider::codes::PROGRAM_NOT_FOUND,
            format!("The {noun} `{at}` is not there, is not a file, or cannot be run."),
            "Install the program at the path its surface declares, or correct the path in `target.yaml`.",
        ),
        ProgramRefused::NotAbsolute => unsafe_path(
            format!("The {noun} `{at}` is not named by an absolute path."),
            "Name the program by its absolute path in `target.yaml`; it is never searched for.",
        ),
        ProgramRefused::Script => unsafe_path(
            format!(
                "The {noun} `{at}` is a shell, an interpreter, a launcher, package runner, container runner, or build tool that runs code its arguments name, a batch file, or a script that one of them would run."
            ),
            "Name a program that is itself the server, or, for a server a launcher runs, name a launcher of the provider's table as `program` and the server's installed entry file as `entry`, with `entry_digest` (decision 0399).",
        ),
        ProgramRefused::Writable(what) => unsafe_path(
            format!(
                "The {noun} `{at}` could be replaced by another user: `{}` is theirs to write.",
                place(what)
            ),
            "Give the file and every folder above it to the engine's user or the administrator, with no write access for anyone else.",
        ),
        ProgramRefused::Unreadable(what) => unsafe_path(
            format!(
                "The {noun} `{at}` is not started: who may change `{}` could not be read.",
                place(what)
            ),
            "Give the file and every folder above it an access list the engine's user can read, naming only that user and the system's own identities.",
        ),
        ProgramRefused::Link(what) => unsafe_path(
            format!(
                "The {noun} `{at}` is not started: `{}` is a link, which is not followed on this system.",
                place(what)
            ),
            "Name the program by the path it has with no link on the way.",
        ),
        ProgramRefused::Elsewhere => unsafe_path(
            format!(
                "The {noun} `{at}` is on another machine's share, a device, a drive that is not fixed in this machine, or a drive letter that stands for a folder."
            ),
            "Install the program on a local drive and name it by that path.",
        ),
    }
}

/// A failure before anything was sent.
pub(crate) fn unsent(failure: Failure) -> StepFailure {
    StepFailure::not_sent(failure)
}

/// A name a server or a file gave, made safe to show: only the characters a
/// tool's name is made of are kept, and it is cut short. Text that is not
/// such a name is never shown.
pub(crate) fn shown(name: &str) -> String {
    kept(name, &['_', '-', '.', '/'])
}

/// A path the published plan declares, made safe to show as [`shown`] makes
/// a name, keeping too the backslash and the colon of a Windows path, so
/// that `C:\Programs\example-mcp.exe` is shown as itself.
pub(crate) fn shown_path(path: &str) -> String {
    kept(path, &['_', '-', '.', '/', '\\', ':'])
}

/// `text` with only ASCII letters, digits, and `also` kept, cut short.
fn kept(text: &str, also: &[char]) -> String {
    let plain: String = text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || also.contains(character))
        .take(96)
        .collect();
    if plain.len() == text.len() {
        plain
    } else {
        format!("{plain}…")
    }
}

#[cfg(test)]
mod tests {
    use super::{shown, shown_path};

    #[test]
    fn a_windows_path_is_shown_as_itself_and_a_name_keeps_only_a_names_characters() {
        let path = r"D:\a\example\target\debug\example-mcp.exe";
        assert_eq!(shown_path(path), path);
        assert_eq!(
            shown_path("/opt/example/bin/example-mcp"),
            "/opt/example/bin/example-mcp"
        );
        assert_eq!(shown_path("C:\\x`y"), "C:\\xy…");
        assert_eq!(shown(path), "Daexampletargetdebugexample-mcp.exe…");
    }
}
