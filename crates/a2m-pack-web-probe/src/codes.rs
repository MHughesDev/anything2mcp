//! The failure codes this crate produces. Each belongs to exactly one
//! class, and the manifest declares each.

a2m_core::failure_codes! {
    FORBIDDEN_BY_EDGE = EdgeBlocked, "forbidden_by_edge",
        meaning "The surface answered a page with status 403.",
        next "Look at the surface in a browser, and ask its owner before trying again.";
    RATE_LIMITED_BY_EDGE = EdgeBlocked, "rate_limited_by_edge",
        meaning "The surface answered a page with status 429.",
        next "Look at the surface in a browser, and ask its owner before trying again.";
    NEEDS_LOGIN = SessionExpired, "needs_login",
        meaning "The page asks for a sign-in, and discovery looks as nobody.",
        next "Run the probe to see the sign-in method; a page behind a sign-in is looked at only by a session the keeper holds.";
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared",
        meaning "An address outside the target's declared surfaces was asked for.",
        next "Declare the address as a surface of the target in the run's environment, then try again.";
    ADDRESS_LOOKS_LIKE_AN_ACTION = Policy, "address_looks_like_an_action",
        meaning "Discovery was to visit an address that looks like an action.",
        next "Leave the address out of the focus, or look at the page in a browser yourself.";
    FOCUS_INVALID = Input, "focus_invalid",
        meaning "The focus of a discovery is not in the form this pack reads.",
        next "Give paths of the surface, such as `/orders`.";
    PAGE_UNREADABLE = UnknownVariant, "page_unreadable",
        meaning "A page could not be read, in a way no rule covers.",
        next "Look at the page in a browser; report this if the page is an ordinary one.";
    PAGE_NOT_LOADED = Transient, "page_not_loaded",
        meaning "A page did not load.",
        next "Check that the surface's address is right and that the system is up, then run discovery again.";
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
