//! Runs the built `web2mcp` binary, to prove it starts on each operating system.

use std::error::Error;
use std::process::Command;

fn web2mcp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_web2mcp"))
}

#[test]
fn prints_its_version_with_no_arguments() -> Result<(), Box<dyn Error>> {
    let output = web2mcp().output()?;
    assert!(output.status.success());
    let expected = format!("web2mcp {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(String::from_utf8(output.stdout)?.trim_end(), expected);
    Ok(())
}

#[test]
fn prints_its_version_when_asked() -> Result<(), Box<dyn Error>> {
    for flag in ["--version", "-V"] {
        let output = web2mcp().arg(flag).output()?;
        assert!(output.status.success(), "{flag} failed");
        let expected = format!("web2mcp {}", env!("CARGO_PKG_VERSION"));
        assert_eq!(String::from_utf8(output.stdout)?.trim_end(), expected);
    }
    Ok(())
}

#[test]
fn refuses_a_command_that_does_not_exist() -> Result<(), Box<dyn Error>> {
    let output = web2mcp().arg("serve").output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("no commands exist yet"));
    Ok(())
}
