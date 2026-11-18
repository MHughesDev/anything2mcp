//! Finding a browser program and saying how it is started. Starting it is
//! the supervisor's; this module only gives the supervisor its data: the
//! program, the fixed argument list, the environment, and how the program
//! takes its pipe (decision 0070).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use a2m_core::{Failure, FailureClass};
use a2m_provider::{CheckedPath, EnvAllowlist, ForeignPipe};

/// One place a browser is installed, on one operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownPlace {
    /// The operating system, as `std::env::consts::OS` names it.
    pub os: &'static str,
    /// The absolute path of the program.
    pub path: &'static str,
}

/// The fixed list of places where the stable Chrome, Edge, and Chromium
/// are installed, in the order they are looked for. Data, never a search:
/// no search path, no registry, no environment variable.
///
/// The manifest of the pack that uses this crate must list the same places
/// under a `[[requirements]]` entry, because the supervisor starts only the
/// programs a manifest declares.
pub const KNOWN_PLACES: &[KnownPlace] = &[
    KnownPlace {
        os: "linux",
        path: "/opt/google/chrome/chrome",
    },
    KnownPlace {
        os: "linux",
        path: "/opt/microsoft/msedge/msedge",
    },
    KnownPlace {
        os: "linux",
        path: "/usr/lib/chromium/chromium",
    },
    KnownPlace {
        os: "linux",
        path: "/usr/lib/chromium-browser/chromium-browser",
    },
    KnownPlace {
        os: "linux",
        path: "/usr/lib64/chromium-browser/chromium-browser",
    },
    KnownPlace {
        os: "macos",
        path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    },
    KnownPlace {
        os: "macos",
        path: "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    },
    KnownPlace {
        os: "macos",
        path: "/Applications/Chromium.app/Contents/MacOS/Chromium",
    },
    KnownPlace {
        os: "windows",
        path: r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    },
    KnownPlace {
        os: "windows",
        path: r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    },
    KnownPlace {
        os: "windows",
        path: r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    },
    KnownPlace {
        os: "windows",
        path: r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    },
];

/// The known places of this operating system, in order.
pub fn known_places() -> impl Iterator<Item = &'static str> {
    KNOWN_PLACES
        .iter()
        .filter(|place| place.os == std::env::consts::OS)
        .map(|place| place.path)
}

/// The browser program to use: the one named in settings when there is
/// one, and otherwise the first known place that holds a file. A program
/// named in settings that is not there is not replaced by another: the
/// person named it.
pub(crate) fn find(named: Option<&Path>) -> Option<PathBuf> {
    match named {
        Some(program) => program.is_file().then(|| program.to_path_buf()),
        None => known_places()
            .map(PathBuf::from)
            .find(|path| path.is_file()),
    }
}

/// Makes a browser's data directory private to the engine's user, and
/// refuses one that is not private afterwards.
///
/// The browser is started with a key store of its own, so the key its
/// cookie store is written under is fixed and known: who can read the
/// folder can read the session. On Linux and macOS the folder is given to
/// its owner alone, mode 0700, whatever the process's mask left it with;
/// a folder that belongs to someone else cannot be changed and is refused.
/// The browser gives its own files the modes 0600 and 0700 (measured,
/// decision 0143), so this folder is the second lock, not the only one.
///
/// On Windows nothing is done here. A folder takes the access list of the
/// folder it is made in; under a person's own profile that is the person,
/// the system, and the administrators. The engine does not read access
/// lists, so a data directory placed where others can read is not noticed
/// by this crate.
pub(crate) fn make_private(folder: &CheckedPath) -> Result<(), Failure> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = folder.as_path();
        let private = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .and_then(|()| std::fs::metadata(path))
            .is_ok_and(|now| is_private(now.permissions().mode()));
        if !private {
            return Err(Failure::new(
                FailureClass::Input,
                a2m_provider::codes::PATH_NOT_ALLOWED,
                format!(
                    "The browser's data directory `{folder}` can be read by other users of this machine, and the engine could not make it private."
                ),
                "Give the folder to the engine's user alone (mode 0700), or remove it so that the engine makes it afresh.",
            ));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (folder, FailureClass::Input);
    }
    Ok(())
}

/// Whether a folder's mode lets nobody but its owner in.
#[cfg(unix)]
fn is_private(mode: u32) -> bool {
    mode & 0o077 == 0
}

/// The arguments every browser is started with, before its data directory.
/// A fixed list: nothing from a target file, a tool input, or a page can
/// add to it. It never holds a flag that switches the sandbox off, hides
/// that the browser is automated, or alters what the browser reports
/// about itself.
pub const FIXED_ARGUMENTS: &[&str] = &[
    // The protocol on an inherited pipe, never a port.
    "--remote-debugging-pipe",
    // Says, to the page and to a person, that the browser is automated.
    "--enable-automation",
    "--no-first-run",
    "--no-default-browser-check",
    // The browser's own background traffic: updates, sync, and reports.
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-sync",
    "--disable-default-apps",
    // Every connection is direct (decision 0197): no proxy from the
    // environment, the operating system's settings, automatic discovery,
    // or the profile's stored settings, all of which this flag outranks
    // (decision 0201). Loopback is never sent to a proxy anyway.
    "--no-proxy-server",
    // No extension runs, so none can set a proxy, which would outrank the
    // flag above: an extension another program installs into every
    // profile included.
    "--disable-extensions",
    // The data directory keeps its own key: the operating system's key
    // store, which holds the person's own browser's key, is never asked.
    "--password-store=basic",
    "--use-mock-keychain",
];

/// The argument that keeps the browser without a window.
pub(crate) const HEADLESS: &str = "--headless=new";

/// The whole argument list of one launch.
pub(crate) fn arguments(data_directory: &Path, window: bool) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = FIXED_ARGUMENTS.iter().map(OsString::from).collect();
    if !window {
        arguments.push(HEADLESS.into());
    }
    let mut directory = OsString::from("--user-data-dir=");
    directory.push(data_directory);
    arguments.push(directory);
    // The page the browser opens with, so that it opens no other.
    arguments.push("about:blank".into());
    arguments
}

/// How the browser takes its pipe: descriptors 3 and 4 on Linux and macOS,
/// two inherited handles named by value on Windows.
pub(crate) fn pipe() -> ForeignPipe {
    ForeignPipe {
        child_reads_fd: 3,
        child_writes_fd: 4,
        windows_argument: "--remote-debugging-io-pipes={read},{write}".to_owned(),
    }
}

/// The environment of the browser. The supervisor clears everything else.
/// On Windows a program cannot start without the system's own folders; on
/// macOS the temporary folder is per person. None of these is a secret or
/// a setting of the engine.
pub(crate) fn environment() -> EnvAllowlist {
    let pass: &[&str] = if cfg!(windows) {
        &[
            "SystemRoot",
            "SystemDrive",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "LOCALAPPDATA",
        ]
    } else if cfg!(target_os = "macos") {
        &["TMPDIR", "HOME"]
    } else {
        &[]
    };
    EnvAllowlist {
        pass: pass.iter().map(|name| (*name).to_owned()).collect(),
        set: Vec::new(),
    }
}

/// What a browser with a window is given of the engine's environment so
/// that it can show one: the variables that name the screen, passed as
/// they are. `None` on Linux when neither an X display nor a Wayland
/// display is named; on Windows and macOS a program reaches the person's
/// desktop with none of them. `named`, from a test, is the X display to use
/// in place of the engine's own.
pub(crate) fn screen(named: Option<&str>) -> Option<EnvAllowlist> {
    let mut screen = EnvAllowlist {
        pass: Vec::new(),
        set: Vec::new(),
    };
    if !cfg!(unix) || cfg!(target_os = "macos") {
        return Some(screen);
    }
    if let Some(display) = named {
        screen
            .set
            .push(("DISPLAY".to_owned(), OsString::from(display)));
        return Some(screen);
    }
    let named = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    if !named("DISPLAY") && !named("WAYLAND_DISPLAY") {
        return None;
    }
    for name in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XAUTHORITY",
    ] {
        screen.pass.push(name.to_owned());
    }
    Some(screen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_arguments_never_switch_the_sandbox_off_or_hide_automation() {
        let all = arguments(Path::new("profile"), false)
            .into_iter()
            .chain(arguments(Path::new("profile"), true))
            .map(|argument| argument.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>();
        for refused in [
            "no-sandbox",
            "disable-setuid-sandbox",
            "disable-web-security",
            "automationcontrolled",
            "disable-blink-features",
            "user-agent",
            "disable-infobars",
            "remote-debugging-port",
            "remote-allow-origins",
            "ignore-certificate-errors",
            "allow-running-insecure-content",
            "disable-site-isolation",
        ] {
            assert!(
                !all.iter().any(|argument| argument.contains(refused)),
                "the launch arguments hold `{refused}`"
            );
        }
        // The protocol travels on the pipe, and the browser says it is automated.
        assert!(
            all.iter()
                .any(|argument| argument == "--remote-debugging-pipe")
        );
        assert!(all.iter().any(|argument| argument == "--enable-automation"));
        // The data directory is always named: the browser's own profile never is.
        assert!(
            all.iter()
                .any(|argument| argument == "--user-data-dir=profile")
        );
    }

    #[test]
    fn every_launch_connects_directly_and_runs_no_extension() {
        for window in [false, true] {
            let all = arguments(Path::new("profile"), window)
                .into_iter()
                .map(|argument| argument.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>();
            assert!(all.iter().any(|argument| argument == "--no-proxy-server"));
            assert!(
                all.iter()
                    .any(|argument| argument == "--disable-extensions")
            );
            // Nothing names a proxy, a way to find one, or an extension.
            for named in [
                "--proxy-server",
                "--proxy-pac-url",
                "--proxy-auto-detect",
                "--proxy-bypass-list",
                "--winhttp-proxy-resolver",
                "--load-extension",
                "--disable-extensions-except",
            ] {
                assert!(
                    !all.iter().any(|argument| argument.starts_with(named)),
                    "the launch arguments hold `{named}`"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_is_private_only_when_nobody_but_its_owner_has_any_right_in_it() {
        assert!(is_private(0o40700));
        assert!(is_private(0o40500));
        for open in [0o40755, 0o40750, 0o40701, 0o40710, 0o40770, 0o40707] {
            assert!(!is_private(open), "{open:o}");
        }
    }

    #[test]
    fn every_known_place_is_a_fixed_absolute_path_of_one_operating_system() {
        for place in KNOWN_PLACES {
            assert!(["linux", "macos", "windows"].contains(&place.os));
            let fixed = match place.os {
                "windows" => place.path.starts_with(r"C:\") && place.path.ends_with(".exe"),
                _ => place.path.starts_with('/'),
            };
            assert!(fixed, "{}", place.path);
            assert!(!place.path.contains(".."));
        }
        for os in ["linux", "macos", "windows"] {
            assert!(KNOWN_PLACES.iter().any(|place| place.os == os));
        }
    }

    #[test]
    fn a_program_named_in_settings_is_never_replaced_by_another() {
        let nowhere = std::env::temp_dir().join("a2m-no-such-browser-program");
        assert_eq!(find(Some(&nowhere)), None);
        let this = std::env::current_exe().unwrap();
        assert_eq!(find(Some(&this)), Some(this));
    }
}
