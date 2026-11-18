//! The failure codes this crate produces. Each belongs to exactly one
//! class, and the manifest declares each.

use a2m_core::{FailureClass, FailureCode};

macro_rules! codes {
    ($( $(#[$doc:meta])* $name:ident = $class:ident, $text:literal; )+) => {
        $( $(#[$doc])* pub const $name: FailureCode = FailureCode::fixed($text); )+

        /// Every code of this crate, with its class.
        pub const ALL: &[(FailureClass, &str)] = &[ $( (FailureClass::$class, $text) ),+ ];
    };
}

codes! {
    /// The surface answered a page with status 403.
    FORBIDDEN_BY_EDGE = EdgeBlocked, "forbidden_by_edge";
    /// The surface answered a page with status 429.
    RATE_LIMITED_BY_EDGE = EdgeBlocked, "rate_limited_by_edge";
    /// The page asks for a sign-in, and discovery looks as nobody.
    NEEDS_LOGIN = SessionExpired, "needs_login";
    /// Discovery was asked to look at an address outside the surface.
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared";
    /// Discovery was to visit an address that looks like an action.
    ADDRESS_LOOKS_LIKE_AN_ACTION = Policy, "address_looks_like_an_action";
    /// The focus of a discovery is not in the form this pack reads.
    FOCUS_INVALID = Input, "focus_invalid";
    /// A page could not be read, in a way no rule covers.
    PAGE_UNREADABLE = UnknownVariant, "page_unreadable";
    /// A page did not load.
    PAGE_NOT_LOADED = Transient, "page_not_loaded";
}

/// Text from outside, cut before it is shown in a message.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 160;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}
