//! A session bus of the test's own, and the accessibility bus behind it.
//!
//! The test never touches the machine's real session: the bus daemon runs
//! with a configuration of the stand's own, listening in a folder of the
//! stand's own, and every program it starts on demand (the accessibility
//! bus launcher, which starts the accessibility bus and the registry)
//! inherits an environment with no display, a runtime folder of the
//! stand's own, and settings kept in memory. So nothing the test does
//! reaches a person's display, their accessibility bus, or their settings.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{REQUIRE_ENV, StandError, required};

/// The service file through which a session bus starts the accessibility
/// bus launcher. Without it no accessibility bus can be had.
const LAUNCHER_SERVICE: &str = "/usr/share/dbus-1/services/org.a11y.Bus.service";

/// A private session bus. Dropping it stops the bus, which ends the
/// accessibility bus and the registry it started.
pub struct Session {
    child: Child,
    address: String,
    folder: PathBuf,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// The bus daemon, when this machine has it and the accessibility bus
/// launcher's service.
fn programs() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") || !Path::new(LAUNCHER_SERVICE).is_file() {
        return None;
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join("dbus-daemon"))
        .find(|path| path.is_file())
}

fn config(folder: &Path) -> String {
    format!(
        r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <standard_session_servicedirs/>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
        folder.display()
    )
}

impl Session {
    /// A session bus for one test, or `None` when this machine cannot give
    /// one: not Linux, no bus daemon, or no accessibility bus launcher.
    ///
    /// # Errors
    ///
    /// When the programs are there and the bus does not start, or when they
    /// are not there and `A2M_REQUIRE_DESKTOP=1` says they must be.
    pub fn start() -> Result<Option<Self>, StandError> {
        let Some(daemon) = programs() else {
            if required() {
                return Err(StandError(format!(
                    "the bus daemon and the accessibility bus launcher ({LAUNCHER_SERVICE}) were not found, and {REQUIRE_ENV}=1 says they must be"
                )));
            }
            return Ok(None);
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "example-desk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let made = |doing: &str| {
            let doing = doing.to_owned();
            move |error: std::io::Error| StandError(format!("{doing}: {error}"))
        };
        let runtime = folder.join("runtime");
        std::fs::create_dir_all(&runtime).map_err(made("making the session's folder"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700))
                .map_err(made("closing the runtime folder"))?;
        }
        let conf = folder.join("session.conf");
        std::fs::write(&conf, config(&folder)).map_err(made("writing the bus's configuration"))?;
        let mut child = Command::new(daemon)
            .arg(format!("--config-file={}", conf.display()))
            .arg("--nofork")
            .arg("--print-address=1")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("AT_SPI_BUS_ADDRESS")
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_CONFIG_HOME", folder.join("config"))
            .env("XDG_CACHE_HOME", folder.join("cache"))
            .env("GSETTINGS_BACKEND", "memory")
            .env("NO_AT_BRIDGE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(made("starting the bus daemon"))?;
        let mut line = String::new();
        let read = child
            .stdout
            .take()
            .map(|out| BufReader::new(out).read_line(&mut line));
        let address = line.trim().to_owned();
        if !matches!(read, Some(Ok(_))) || address.is_empty() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&folder);
            return Err(StandError(
                "the bus daemon started and printed no address".to_owned(),
            ));
        }
        Ok(Some(Self {
            child,
            address,
            folder,
        }))
    }

    /// The bus's address, for a client of this session only.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}
