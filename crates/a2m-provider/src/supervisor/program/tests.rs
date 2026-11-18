//! The tests of the check of who may change a program, moved here with the
//! check from the command pack (decision 0250), and the tests of the closed
//! lists kept beside it.
use std::path::Path;

use super::{ProgramRefused as Refused, ProgramRole as Role, ProgramTrust as Trust};

/// The check of a program, with no group named.
fn check(path: &Path) -> Result<(), Refused> {
    super::check_program(path, &Trust::default(), Role::Program)
}

/// How a program of this system begins: what a test writes for a program
/// it makes.
#[cfg(target_os = "macos")]
const HEAD: &[u8] = &[0xcf, 0xfa, 0xed, 0xfe];
#[cfg(windows)]
const HEAD: &[u8] = b"MZ";
#[cfg(not(any(target_os = "macos", windows)))]
const HEAD: &[u8] = &[0x7f, b'E', b'L', b'F'];

#[test]
fn a_relative_path_a_missing_file_and_a_shell_are_refused() {
    assert_eq!(check(Path::new("example-tool")), Err(Refused::NotAbsolute));
    let missing = std::env::temp_dir().join("a2m-provider-no-such-program");
    assert_eq!(check(&missing), Err(Refused::Missing));
    assert_eq!(check(&std::env::temp_dir()), Err(Refused::Missing));
    #[cfg(unix)]
    assert_eq!(check(Path::new("/bin/sh")), Err(Refused::Script));
}

#[cfg(unix)]
#[test]
fn a_script_and_a_file_others_can_write_are_refused() {
    use std::os::unix::fs::PermissionsExt as _;

    let folder = std::env::temp_dir().join(format!("a2m-provider-check-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let script = folder.join("example-tool");
    std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(check(&script), Err(Refused::Script));

    let open = folder.join("example-open");
    std::fs::write(&open, HEAD).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(check(&open), Err(Refused::Writable(path)) if path.ends_with("example-open")));

    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(check(&open), Err(Refused::Missing), "it cannot be run");
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A folder of the test's own with a program in `real/`, and a way to
/// make more folders with a mode.
#[cfg(unix)]
struct Place(std::path::PathBuf);

#[cfg(unix)]
impl Place {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("a2m-provider-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // As the system names it, so that a refusal's path can be
        // compared: the temporary folder is behind a link on macOS.
        let place = Self(std::fs::canonicalize(&root).unwrap());
        place.folder("", 0o755);
        place.folder("real", 0o755);
        let program = place.0.join("real").join("example-tool");
        std::fs::write(&program, HEAD).unwrap();
        Self::mode(&program, 0o755);
        place
    }

    fn mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn folder(&self, name: &str, mode: u32) -> std::path::PathBuf {
        let folder = self.0.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        Self::mode(&folder, mode);
        folder
    }

    fn link(target: &Path, at: &Path) {
        std::os::unix::fs::symlink(target, at).unwrap();
    }
}

#[cfg(unix)]
impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn a_link_in_a_folder_others_can_write_is_refused_and_a_closed_chain_is_followed() {
    let place = Place::new("links");
    let program = place.0.join("real").join("example-tool");
    assert_eq!(check(&program), Ok(()));

    // The declared path is a link in a folder anyone may write: whoever
    // can write the folder chooses where the link leads.
    let open = place.folder("open", 0o777);
    Place::link(&program, &open.join("example-tool"));
    assert_eq!(
        check(&open.join("example-tool")),
        Err(Refused::Writable(open.clone()))
    );
    // The same for an open folder that the path only passes through.
    Place::link(&place.0.join("real"), &open.join("through"));
    assert_eq!(
        check(&open.join("through").join("example-tool")),
        Err(Refused::Writable(open.clone()))
    );

    // A chain that nobody else can write is followed to the file: a
    // link to a link, one of them relative and through `..`.
    let closed = place.folder("closed", 0o755);
    Place::link(
        Path::new("../real/example-tool"),
        &closed.join("example-first"),
    );
    Place::link(&closed.join("example-first"), &closed.join("example-tool"));
    assert_eq!(check(&closed.join("example-tool")), Ok(()));
    // The folder the first link is in is opened afterwards: refused.
    Place::link(&closed.join("example-tool"), &open.join("example-far"));
    assert!(matches!(
        check(&open.join("example-far")),
        Err(Refused::Writable(_))
    ));

    // Where only an entry's owner may replace it, a link of ours holds.
    let shared = place.folder("shared", 0o1777);
    Place::link(&program, &shared.join("example-tool"));
    assert_eq!(check(&shared.join("example-tool")), Ok(()));

    // Links that lead in a circle lead nowhere, and a link to a shell
    // is a shell.
    Place::link(&closed.join("example-b"), &closed.join("example-a"));
    Place::link(&closed.join("example-a"), &closed.join("example-b"));
    assert_eq!(check(&closed.join("example-a")), Err(Refused::Missing));
    Place::link(Path::new("/bin/sh"), &closed.join("example-shell"));
    assert_eq!(check(&closed.join("example-shell")), Err(Refused::Script));
    // A file is the end of a path.
    assert_eq!(check(&program.join("more")), Err(Refused::Missing));
}

#[cfg(unix)]
#[test]
fn a_folder_or_a_file_a_group_can_write_is_refused_unless_the_group_counts() {
    let place = Place::new("group");
    let folder = place.folder("grouped", 0o775);
    let program = folder.join("example-tool");
    std::fs::copy(place.0.join("real").join("example-tool"), &program).unwrap();
    Place::mode(&program, 0o755);
    assert_eq!(check(&program), Err(Refused::Writable(folder.clone())));
    Place::mode(&folder, 0o755);
    assert_eq!(check(&program), Ok(()));
    Place::mode(&program, 0o775);
    assert_eq!(check(&program), Err(Refused::Writable(program.clone())));
}

/// A group-writable folder with a program in it, and the folder's group.
#[cfg(unix)]
fn grouped(place: &Place) -> (std::path::PathBuf, std::path::PathBuf, u32) {
    use std::os::unix::fs::MetadataExt as _;
    let folder = place.folder("grouped", 0o775);
    let program = folder.join("example-tool");
    std::fs::copy(place.0.join("real").join("example-tool"), &program).unwrap();
    Place::mode(&program, 0o775);
    let group = std::fs::metadata(&folder).unwrap().gid();
    (folder, program, group)
}

#[cfg(unix)]
#[test]
fn a_group_the_person_names_counts_as_the_administrators_and_no_other_does() {
    let place = Place::new("named");
    let (folder, program, group) = grouped(&place);
    let named = |groups: Vec<u32>| {
        super::check_program(&program, &Trust::with_groups(groups), Role::Program)
    };
    // Named: the folder and the file that group may write are started.
    assert_eq!(named(vec![group]), Ok(()));
    // Not named, or another group named: refused, as before.
    assert_eq!(named(vec![]), Err(Refused::Writable(folder.clone())));
    assert_eq!(
        named(vec![group.wrapping_add(1)]),
        Err(Refused::Writable(folder.clone()))
    );
    // A group's name never stands for it: only the system's own list
    // and the setting's numbers do. Others' write is refused whatever
    // group is named.
    Place::mode(&folder, 0o777);
    assert_eq!(named(vec![group]), Err(Refused::Writable(folder.clone())));
}

#[cfg(target_os = "linux")]
#[test]
fn on_linux_no_group_counts_by_default() {
    let place = Place::new("linux");
    let (folder, program, _) = grouped(&place);
    assert_eq!(check(&program), Err(Refused::Writable(folder)));
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_the_administrators_group_counts_and_staff_does_not() {
    let place = Place::new("macos");
    let (folder, program, _) = grouped(&place);
    // Giving a file to a group needs no privilege when the test's user
    // is in it; the runner's user is an administrator, but a machine
    // where it is not cannot prove this.
    for (group, path) in [(80, &folder), (80, &program)] {
        if let Err(error) = std::os::unix::fs::chown(path, None, Some(group)) {
            println!("skipped: this user cannot give a file to the group {group}: {error}");
            return;
        }
    }
    assert_eq!(check(&program), Ok(()));
    if let Err(error) = std::os::unix::fs::chown(&folder, None, Some(20)) {
        println!("skipped: this user cannot give a file to the group 20: {error}");
        return;
    }
    assert_eq!(check(&program), Err(Refused::Writable(folder)));
}

/// A folder the engine made, and a file it wrote there: refused when
/// another user could change either, or a folder above them (PR-4 of the
/// fifth safety audit, decision 0250).
#[cfg(unix)]
#[test]
fn a_folder_of_the_engines_and_a_file_it_wrote_are_refused_when_others_can_change_them() {
    let place = Place::new("folder");
    let runs = place.folder("runs", 0o700);
    let copy = runs.join("example-copy");
    std::fs::write(&copy, "echo hi\n").unwrap();
    Place::mode(&copy, 0o600);
    let folder = |path: &Path| super::check_program(path, &Trust::default(), Role::Folder);
    let script = |path: &Path| super::check_program(path, &Trust::default(), Role::Script);
    assert_eq!(folder(&runs), Ok(()));
    // A script is neither run nor refused for its first bytes or its name.
    assert_eq!(script(&copy), Ok(()));
    assert_eq!(folder(&copy), Err(Refused::Missing), "a file is no folder");
    assert_eq!(script(&runs), Err(Refused::Missing), "a folder is no file");
    // The group may write the folder: the copy could be replaced in it.
    Place::mode(&runs, 0o770);
    assert_eq!(folder(&runs), Err(Refused::Writable(runs.clone())));
    assert_eq!(script(&copy), Err(Refused::Writable(runs.clone())));
    Place::mode(&runs, 0o700);
    // Others may write the copy itself.
    Place::mode(&copy, 0o666);
    assert_eq!(script(&copy), Err(Refused::Writable(copy.clone())));
    // Others may write the folder above the run's folder.
    Place::mode(&copy, 0o600);
    Place::mode(&place.0, 0o777);
    assert_eq!(folder(&runs), Err(Refused::Writable(place.0.clone())));
    Place::mode(&place.0, 0o755);
}

#[test]
fn only_an_interpreter_of_the_closed_table_in_its_own_form_is_started() {
    use super::{InterpreterRefused, interpreter_form};
    let given = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    // Each form of the table, by every name it is known by.
    for path in ["/bin/sh", "/usr/bin/dash", "/bin/bash"] {
        assert_eq!(interpreter_form(path, &given(&[])), Ok(()), "{path}");
    }
    assert_eq!(
        interpreter_form("/usr/bin/python3.12", &given(&["-I"])),
        Ok(())
    );
    assert_eq!(
        interpreter_form(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            &given(&[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File"
            ])
        ),
        Ok(())
    );
    assert_eq!(interpreter_form("/usr/bin/osascript", &given(&[])), Ok(()));
    // An empty list is no safer than another: Python without `-I` loads
    // what its variables and the script's folder name, and PowerShell
    // without `-File` reads the rest of its line as a command.
    assert!(matches!(
        interpreter_form("/usr/bin/python3", &given(&[])),
        Err(InterpreterRefused::NotItsForm(["-I"]))
    ));
    assert!(matches!(
        interpreter_form(r"C:\tools\powershell.exe", &given(&["-NoProfile"])),
        Err(InterpreterRefused::NotItsForm(_))
    ));
    // A flag before the script changes what the shell reads.
    for flag in ["-c", "-s", "-i", "-x"] {
        assert!(
            matches!(
                interpreter_form("/bin/sh", &given(&[flag])),
                Err(InterpreterRefused::NotItsForm([]))
            ),
            "{flag}"
        );
    }
    // The command interpreters that read their whole line as code, and a
    // batch file, by name, in any spelling.
    for path in [
        r"C:\Windows\System32\cmd.exe",
        r"C:\Windows\System32\CMD.EXE",
        r"C:\Windows\command.com",
        r"C:\Windows\System32\wscript.exe",
        r"C:\Windows\System32\cscript.exe",
        r"C:\Windows\System32\mshta.exe",
        r"C:\tools\run.bat",
        r"C:\tools\run.CMD",
    ] {
        assert_eq!(
            interpreter_form(path, &given(&["/r"])),
            Err(InterpreterRefused::ReadsItsLineAsCode),
            "{path}"
        );
        assert_eq!(
            interpreter_form(path, &given(&[])),
            Err(InterpreterRefused::ReadsItsLineAsCode),
            "{path}"
        );
    }
    // Every other interpreter, and a program that is none, until a test
    // proves its form.
    for path in [
        "/usr/bin/node",
        "/usr/bin/perl",
        "/usr/bin/zsh",
        "/usr/bin/pwsh",
        "/opt/example/bin/example-tool",
    ] {
        assert_eq!(
            interpreter_form(path, &given(&[])),
            Err(InterpreterRefused::NotKnown),
            "{path}"
        );
    }
}

#[test]
fn an_interpreter_that_reads_its_line_as_code_is_refused_by_name_as_an_interpreter() {
    let check =
        |path: &str| super::check_program(Path::new(path), &Trust::default(), Role::Interpreter);
    #[cfg(windows)]
    for path in [r"C:\Windows\System32\cmd.exe", r"C:\tools\run.bat"] {
        assert_eq!(check(path), Err(Refused::Script), "{path}");
    }
    #[cfg(unix)]
    for path in ["/opt/example/cmd", "/opt/example/run.bat"] {
        assert_eq!(check(path), Err(Refused::Script), "{path}");
    }
}

#[test]
fn a_variable_that_makes_a_program_load_other_code_is_known_by_its_name() {
    use super::redirects_loading;
    for name in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "ld_preload",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PYTHONPATH",
        "PYTHONSTARTUP",
        "PYTHONHOME",
        "PERL5OPT",
        "PERL5LIB",
        "RUBYOPT",
        "RUBYLIB",
        "BASH_ENV",
        "BASH_FUNC_example%%",
        "ENV",
        "SHELLOPTS",
        "PS4",
        "GCONV_PATH",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "CLASSPATH",
        "LUA_INIT",
        "PSModulePath",
        "COR_PROFILER",
        "CORECLR_PROFILER_PATH",
        "DOTNET_STARTUP_HOOKS",
    ] {
        assert!(redirects_loading(name), "{name}");
    }
    for name in [
        "EXAMPLE_MCP_STATE",
        "NODE_ENV",
        "PATH",
        "HOME",
        "LANG",
        "TZ",
        "SystemRoot",
        "ENVIRONMENT",
        "LDAP_HOST",
    ] {
        assert!(!redirects_loading(name), "{name}");
    }
}

#[test]
fn shells_interpreters_and_programs_that_start_another_are_known_by_name() {
    use super::is_shell_or_interpreter;
    for refused in [
        "/bin/sh",
        "/usr/bin/python3.12",
        "/usr/bin/Rscript",
        "/usr/bin/java",
        "/usr/bin/timeout",
        r"C:\Windows\System32\rundll32.exe",
        r"C:\Windows\System32\WSL.EXE",
        r"C:\tools\run.bat",
    ] {
        assert!(is_shell_or_interpreter(refused), "{refused}");
    }
    for allowed in ["/opt/example/bin/example-tool", "/usr/bin/finder"] {
        assert!(!is_shell_or_interpreter(allowed), "{allowed}");
    }
}

/// Finding D6 of the sixth safety audit (decision 0271): a file that can be
/// run and does not begin as this system's programs do is a script, which
/// the C library hands to the shell when the system will not run it. It is
/// refused as a program and as an interpreter; a file the engine wrote is
/// not looked at for its first bytes.
#[cfg(unix)]
#[test]
fn a_file_that_does_not_begin_as_a_program_of_this_system_is_refused_as_a_script() {
    let place = Place::new("header");
    let folder = place.0.join("real");
    for (name, bytes) in [
        ("example-text", b"echo hi\n".as_slice()),
        ("example-short", b"\x7f".as_slice()),
        ("example-empty", b"".as_slice()),
        ("example-pe", b"MZ\x90\x00".as_slice()),
    ] {
        let path = folder.join(name);
        std::fs::write(&path, bytes).unwrap();
        Place::mode(&path, 0o755);
        assert_eq!(check(&path), Err(Refused::Script), "{name}");
        assert_eq!(
            super::check_program(&path, &Trust::default(), Role::Interpreter),
            Err(Refused::Script),
            "{name}"
        );
        assert_eq!(
            super::check_program(&path, &Trust::default(), Role::Script),
            Ok(()),
            "{name}"
        );
    }
    // The header of the other Unix system is not this one's.
    #[cfg(target_os = "macos")]
    let other: &[u8] = &[0x7f, b'E', b'L', b'F'];
    #[cfg(not(target_os = "macos"))]
    let other: &[u8] = &[0xcf, 0xfa, 0xed, 0xfe];
    let path = folder.join("example-other");
    std::fs::write(&path, other).unwrap();
    Place::mode(&path, 0o755);
    assert_eq!(check(&path), Err(Refused::Script));
}

/// The same on Windows: a file without the header every Windows program
/// has is not a program.
#[cfg(windows)]
#[test]
fn a_file_without_the_header_of_a_windows_program_is_refused_as_a_script() {
    let root = std::env::temp_dir().join(format!("a2m-provider-header-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let text = root.join("example-tool.exe");
    std::fs::write(&text, b"echo hi\r\n").unwrap();
    assert_eq!(check(&text), Err(Refused::Script));
    std::fs::write(&text, HEAD).unwrap();
    assert_eq!(check(&text), Ok(()));
    let _ = std::fs::remove_dir_all(&root);
}

/// On Linux the group's bits of the mode bound what a named user of an
/// access list may hold. Where the group's write is accepted because the
/// group counts as the administrators', a named user's write behind those
/// bits is refused (finding D3, decision 0271).
#[cfg(target_os = "linux")]
#[test]
fn a_named_users_write_behind_a_trusted_groups_bits_is_refused_on_linux() {
    let place = Place::new("posix-list");
    let (folder, program, group) = grouped(&place);
    Place::mode(&program, 0o755);
    let trusted =
        |path: &Path| super::check_program(path, &Trust::with_groups(vec![group]), Role::Program);
    assert_eq!(trusted(&program), Ok(()));
    // The list in the kernel's own form: owner rwx, the user 65534 rwx,
    // the owning group r-x, the mask rwx, others r-x.
    let mut list = 2_u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01_u16, 7_u16, u32::MAX),
        (0x02, 7, 65_534),
        (0x04, 5, u32::MAX),
        (0x10, 7, u32::MAX),
        (0x20, 5, u32::MAX),
    ] {
        list.extend(tag.to_le_bytes());
        list.extend(perm.to_le_bytes());
        list.extend(id.to_le_bytes());
    }
    if let Err(error) = rustix::fs::lsetxattr(
        &folder,
        "system.posix_acl_access",
        &list,
        rustix::fs::XattrFlags::empty(),
    ) {
        println!("skipped: this file system keeps no access list here: {error}");
        return;
    }
    assert_eq!(trusted(&program), Err(Refused::Unreadable(folder.clone())));
    // Without the group counted, the group's write refuses it, as before.
    assert_eq!(check(&program), Err(Refused::Writable(folder)));
}

/// Finding D2 of the sixth safety audit (decision 0271): the programs that
/// start an interpreter, a package, an image, or the code of a project, by
/// argument, are refused by name, as a version on the end of the name or
/// after it does not hide one.
#[test]
fn launchers_package_runners_containers_and_build_tools_are_known_by_name() {
    use super::is_shell_or_interpreter;
    for refused in [
        r"C:\Windows\py.exe",
        r"C:\Windows\pyw.exe",
        "/usr/local/bin/uvx",
        "/usr/bin/pipx",
        "/usr/local/bin/npx",
        "/usr/local/bin/bunx",
        "/usr/bin/dotnet",
        "/usr/bin/docker",
        "/usr/bin/podman",
        "/usr/local/go/bin/go",
        "/opt/example/.cargo/bin/cargo",
        "/usr/bin/make",
        "/usr/bin/wine",
        r"C:\Windows\System32\msiexec.exe",
        r"C:\Windows\System32\regsvr32.exe",
        r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\MSBuild.exe",
        r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\InstallUtil.exe",
        r"C:\Windows\System32\cmstp.exe",
        r"C:\Windows\System32\conhost.exe",
        "/usr/bin/open",
        "/usr/bin/swift",
        "/usr/local/bin/julia",
        "/usr/bin/escript",
        "/usr/bin/python3-dbg",
        "/usr/bin/python3.12-config",
        "/usr/bin/perl5.36-thread",
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/ld-musl-x86_64.so.1",
    ] {
        assert!(is_shell_or_interpreter(refused), "{refused}");
    }
    for allowed in [
        "/opt/example/bin/example-tool",
        "/usr/bin/finder",
        "/opt/timeouts",
        "/opt/example/bin/python-example-server",
        "/opt/example/bin/gofer",
        "/opt/example/bin/r2d2",
        "/opt/example/bin/opener",
    ] {
        assert!(!is_shell_or_interpreter(allowed), "{allowed}");
    }
}

/// Finding D1 of the sixth safety audit (decision 0271): the variables
/// that the interpreters of the closed table, the runtimes they run on, and
/// the libraries programs link with, read as the name of code to load or a
/// program to run.
#[test]
fn the_variables_the_interpreters_and_their_runtimes_load_code_from_are_known() {
    use super::redirects_loading;
    for name in [
        // Windows PowerShell's .NET Framework, and .NET.
        "APPDOMAIN_MANAGER_ASM",
        "APPDOMAIN_MANAGER_TYPE",
        "COMPlus_Version",
        "COMPLUS_ETWEnabled",
        "COR_ENABLE_PROFILING",
        "DEVPATH",
        "DOTNET_ADDITIONAL_DEPS",
        "DOTNET_ROOT",
        "DOTNET_SHARED_STORE",
        "COREHOST_TRACEFILE",
        "PSModuleAnalysisCachePath",
        // AppleScript, and the loader on macOS through the service
        // launcher.
        "OSA_LIBRARY_PATH",
        "__XPC_DYLD_INSERT_LIBRARIES",
        "__PYVENV_LAUNCHER__",
        // OpenSSL.
        "OPENSSL_CONF",
        "OPENSSL_ENGINES",
        "OPENSSL_MODULES",
        // What runs "the shell", and helper programs.
        "COMSPEC",
        "ComSpec",
        "SHELL",
        "IFS",
        "LESSOPEN",
        "PAGER",
        "EDITOR",
        "GIT_SSH_COMMAND",
        "GIT_CONFIG_PARAMETERS",
        // The C library and its loader.
        "GLIBC_TUNABLES",
        "LOCPATH",
        "NLSPATH",
        // Other runtimes and module paths.
        "ELECTRON_RUN_AS_NODE",
        "TCL_LIBRARY",
        "GTK_MODULES",
        "GIO_EXTRA_MODULES",
        "QT_PLUGIN_PATH",
        "SASL_PATH",
        "GST_PLUGIN_PATH",
        "VK_LAYER_PATH",
        "ERL_FLAGS",
        "ERL_LIBS",
        "JULIA_LOAD_PATH",
        "R_ENVIRON_USER",
        "R_LIBS_USER",
        "__COMPAT_LAYER",
    ] {
        assert!(redirects_loading(name), "{name}");
    }
    for name in [
        "EXAMPLE_MCP_STATE",
        "NODE_ENV",
        "PATH",
        "SystemRoot",
        "SHELLCHECK_EXAMPLE",
        "OSAKA_EXAMPLE",
        "LANG",
    ] {
        assert!(!redirects_loading(name), "{name}");
    }
}
