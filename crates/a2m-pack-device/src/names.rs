//! The names the pack registers, each checked once when the pack is made.

use a2m_core::{Failure, FileKindName, FindingKind, LintCode, Mode, StepKind, SurfaceKind, Verb};

/// The pack's own name, as its manifest states it.
pub const PACK: &str = "device";

/// Every registered name the pack uses.
#[derive(Debug, Clone)]
pub(crate) struct Names {
    pub mode: Mode,
    pub surface_kind: SurfaceKind,
    pub register_kind: FileKindName,
    pub read: Verb,
    pub update: Verb,
    pub submit: Verb,
    pub open: StepKind,
    pub write_frame: StepKind,
    pub read_frame: StepKind,
    pub expect: StepKind,
    pub identity_finding: FindingKind,
}

impl Names {
    /// Checks and builds the names.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            mode: Mode::new("device")?,
            surface_kind: SurfaceKind::new("device_link")?,
            register_kind: FileKindName::new("register")?,
            read: Verb::new("read")?,
            update: Verb::new("update")?,
            submit: Verb::new("submit")?,
            open: StepKind::new("open")?,
            write_frame: StepKind::new("write_frame")?,
            read_frame: StepKind::new("read_frame")?,
            expect: StepKind::new("expect")?,
            identity_finding: FindingKind::new("device_identity")?,
        })
    }
}

/// A lint code of this pack: its name, a dot, and the rule.
pub(crate) fn lint_code(rule: &str) -> Result<LintCode, Failure> {
    Ok(LintCode::new(&format!("{PACK}.{rule}"))?)
}
