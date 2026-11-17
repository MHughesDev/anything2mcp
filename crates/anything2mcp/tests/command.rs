//! Runs the built binaries, to prove they start on each operating system and
//! that the short name `a2m` is the same program as `anything2mcp`.

use std::error::Error;
use std::process::Command;

/// Both names of the command.
const COMMANDS: [&str; 2] = [
    env!("CARGO_BIN_EXE_anything2mcp"),
    env!("CARGO_BIN_EXE_a2m"),
];

#[test]
fn prints_its_version_with_no_arguments() -> Result<(), Box<dyn Error>> {
    for command in COMMANDS {
        let output = Command::new(command).output()?;
        assert!(output.status.success());
        let expected = format!("anything2mcp {}", env!("CARGO_PKG_VERSION"));
        assert_eq!(String::from_utf8(output.stdout)?.trim_end(), expected);
    }
    Ok(())
}

#[test]
fn prints_its_version_when_asked() -> Result<(), Box<dyn Error>> {
    for command in COMMANDS {
        for flag in ["--version", "-V"] {
            let output = Command::new(command).arg(flag).output()?;
            assert!(output.status.success(), "{flag} failed");
            let expected = format!("anything2mcp {}", env!("CARGO_PKG_VERSION"));
            assert_eq!(String::from_utf8(output.stdout)?.trim_end(), expected);
        }
    }
    Ok(())
}

#[test]
fn refuses_a_command_that_does_not_exist() -> Result<(), Box<dyn Error>> {
    for command in COMMANDS {
        let output = Command::new(command).arg("serve").output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr)?.contains("no commands exist yet"));
    }
    Ok(())
}
