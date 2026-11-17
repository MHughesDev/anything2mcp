//! Test equipment. Compiled only into this crate's own tests and into a
//! crate that asks for the `test-support` feature as a dev-dependency; the
//! product never does (decision 0110).
//!
//! The product finds a browser in settings or in a fixed list of known
//! places, and never starts one without its sandbox. A test machine may
//! keep its browser somewhere else, and may run its tests as the
//! superuser, whom the browser refuses to serve with its sandbox on. This
//! module is the one way a test says so: two environment variables, read
//! here and nowhere in the product.

use std::path::PathBuf;

use crate::browsers::BrowserSettings;
use crate::launch::known_places;

/// Names the browser program for tests, by its absolute path.
pub const BROWSER_VARIABLE: &str = "A2M_TEST_BROWSER";

/// When set to `1`, tests start the browser without its sandbox.
pub const NO_SANDBOX_VARIABLE: &str = "A2M_TEST_BROWSER_NO_SANDBOX";

/// The one argument a test may add to a launch.
pub(crate) const WITHOUT_SANDBOX: &str = "--no-sandbox";

/// The browser program the test environment names, if it names one.
#[must_use]
pub fn browser_program() -> Option<PathBuf> {
    std::env::var_os(BROWSER_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The settings of a test: the defaults, with the program the test
/// environment names and, when it asks, no sandbox.
#[must_use]
pub fn settings() -> BrowserSettings {
    BrowserSettings {
        program: browser_program(),
        without_sandbox: std::env::var(NO_SANDBOX_VARIABLE).is_ok_and(|value| value == "1"),
        ..BrowserSettings::default()
    }
}

/// A `[[requirements]]` entry for the manifest of a test's pack: the known
/// places of this operating system and the program the test environment
/// names. A manifest must declare a program before the supervisor starts
/// it, in a test as anywhere.
#[must_use]
pub fn requirement() -> String {
    let mut places: Vec<String> = known_places().map(str::to_owned).collect();
    if let Some(program) = browser_program() {
        places.push(program.display().to_string());
    }
    // TOML literal strings, so that a path of any operating system is
    // taken as written.
    let places: Vec<String> = places.iter().map(|place| format!("'{place}'")).collect();
    format!(
        "\n[[requirements]]\nname = \"browser\"\ndetect = {{ paths = [{}] }}\nmissing = \"Install Chrome, Edge, or Chromium.\"\n",
        places.join(", ")
    )
}
