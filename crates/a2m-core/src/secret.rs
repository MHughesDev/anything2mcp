//! A value that must never be shown.

use std::fmt;

/// What stands in for a secret wherever one would otherwise be printed.
const REDACTED: &str = "[redacted]";

/// A cookie, a token, a password, or any other value a caller must never see.
///
/// A `Secret` cannot be printed, logged, or serialized: its `Debug` and
/// `Display` show `[redacted]`, and it implements neither `Serialize` nor
/// `Clone`. The only way to the value is [`Secret::expose`], which is named so
/// that every use can be found and reviewed.
///
/// This type stops accidents. It does not replace the redactor, which every
/// secret is also registered with so that a value that reached a system and
/// came back in a response is caught as well.
pub struct Secret(String);

impl Secret {
    /// Wraps a value. From here on it cannot be printed.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for handing to the system it belongs to.
    ///
    /// Never pass the result to a formatter, a logger, a trace, an error
    /// message, or a tool result.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    const VALUE: &str = "correct horse battery staple";

    /// A structure of the kind that ends up in a log line.
    #[derive(Debug)]
    #[allow(
        dead_code,
        reason = "read only through Debug, which is the point of the test"
    )]
    struct Session {
        target: &'static str,
        cookie: Secret,
        tokens: Vec<Secret>,
        refresh: Option<Secret>,
    }

    #[test]
    fn a_secret_is_never_shown_by_any_formatter() {
        let secret = Secret::new(VALUE);
        let shown = [
            format!("{secret}"),
            format!("{secret:?}"),
            format!("{secret:#?}"),
            format!("{secret:>40}"),
            secret.to_string(),
        ];
        for text in shown {
            assert!(!text.contains(VALUE), "{text}");
            assert!(!text.contains("horse"), "{text}");
            assert!(text.contains("[redacted]"), "{text}");
        }
    }

    #[test]
    fn a_secret_inside_another_structure_is_never_shown() {
        let session = Session {
            target: "example",
            cookie: Secret::new(VALUE),
            tokens: vec![Secret::new(VALUE), Secret::new(VALUE)],
            refresh: Some(Secret::new(VALUE)),
        };
        for text in [format!("{session:?}"), format!("{session:#?}")] {
            assert!(!text.contains("horse"), "{text}");
            assert_eq!(text.matches("[redacted]").count(), 4, "{text}");
            assert!(text.contains("example"), "{text}");
        }
    }

    #[test]
    fn the_value_is_reached_only_by_asking_for_it() {
        assert_eq!(Secret::new(VALUE).expose(), VALUE);
    }

    /// These fail to compile, and so fail the build, if `Secret` ever gains a
    /// way to be serialized or copied. Each trait in question is given a
    /// second route to `Probe`; a type that has the trait makes the call
    /// ambiguous.
    #[test]
    fn a_secret_cannot_be_serialized_or_cloned() {
        trait Probe<Marker> {
            fn check() {}
        }
        impl<T: ?Sized> Probe<()> for T {}

        struct Serializes;
        impl<T: ?Sized + serde::Serialize> Probe<Serializes> for T {}
        struct Clones;
        impl<T: Clone> Probe<Clones> for T {}

        <Secret as Probe<_>>::check();
    }
}
