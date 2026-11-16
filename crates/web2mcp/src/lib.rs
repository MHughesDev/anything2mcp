//! The Web2MCP binary and the library facade for embedding the engine.
//!
//! Nothing is implemented yet beyond reporting the version. The crates this one
//! may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`.

/// The version of this build of Web2MCP.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_has_three_numeric_parts() {
        let core = version().split(['-', '+']).next().unwrap_or_default();
        let parts: Vec<&str> = core.split('.').collect();
        assert_eq!(parts.len(), 3, "not three parts: {core}");
        for part in parts {
            assert!(part.parse::<u64>().is_ok(), "not a number: {part}");
        }
    }
}
