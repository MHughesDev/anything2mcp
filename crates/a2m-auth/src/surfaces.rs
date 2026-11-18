//! What a target's plan says about one auth entry's sign-in: the surfaces it
//! signs in to, every address the target declares, and whether the plan is a
//! published version's.
//!
//! A provider that only holds or exchanges a credential needs only the
//! addresses. A provider a pack registers because signing in needs the
//! pack's own surface, such as a sign-in through an application's own
//! pages, needs to know where the surface is and what the pack compiled
//! about it: its address in the session's environment, and the pack's
//! sections of the plan. They are plain data from the same plan a run uses,
//! so the sign-in a person makes and the session a run leases are one
//! (decision 0245).
//!
//! A sign-in is held to the rule every request is held to: it goes only to
//! an address the target declares, and what the target declares counts only
//! once a person has published it (decision 0248). The layer above fills
//! these values from the plan; the session keeper checks them before any
//! request.

use std::collections::BTreeSet;

use a2m_core::{AuthPlan, Environment, PackSection, Plan, SurfacePlan};

/// The addresses a sign-in may send a credential, a token, or the person to:
/// those the target declares for its surfaces in the session's environment.
///
/// It is the same allow-list a run's requests are held to, the surfaces'
/// addresses, compared by origin: an `http` or `https` address is allowed
/// when its scheme, host, and port are those of a declared surface's
/// address. A declared address that is not `http` or `https` allows only
/// itself, as written. Nothing else adds to it: there is no setter, so no
/// input names an address here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignInDestinations {
    /// Each declared address, as written.
    written: BTreeSet<String>,
    /// The origin of each declared `http` or `https` address.
    origins: BTreeSet<String>,
}

impl SignInDestinations {
    /// The addresses given, as the target declares them.
    #[must_use]
    pub fn new(addresses: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::default();
        for address in addresses {
            if let Some(origin) = origin_of(&address) {
                out.origins.insert(origin);
            }
            out.written.insert(address);
        }
        out
    }

    /// Whether `address` may be sent a credential, a token, or the person.
    #[must_use]
    pub fn allows(&self, address: &str) -> bool {
        self.written.contains(address)
            || origin_of(address).is_some_and(|origin| self.origins.contains(&origin))
    }

    /// Every declared address, as written, in order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.written.iter().map(String::as_str)
    }

    /// Whether nothing is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.written.is_empty()
    }
}

/// The origin of an `http` or `https` address, as the parser writes it:
/// scheme, host, and port, with the scheme's own port left out. `None` for
/// anything else, an address with a user part included.
pub(crate) fn origin_of(address: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(address).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let origin = parsed.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

/// The surfaces an auth entry covers, as the plan holds them for one
/// environment, the plan's pack sections for their kinds, every address the
/// target declares there, and how far the plan counts. It holds no
/// credential: the plan never does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntrySurfaces {
    /// Each surface the entry covers, in the entry's order, at the address
    /// it has in the environment.
    pub surfaces: Vec<SurfacePlan>,
    /// The plan's pack sections registered for the kinds of those surfaces,
    /// in the plan's order.
    pub sections: Vec<PackSection>,
    /// Whether the plan is a published version's. A drafts' plan is not:
    /// anyone who can write a draft could name the addresses in it, so the
    /// keeper makes no sign-in, and renews none, on a drafts' word alone
    /// (decision 0248). False unless the caller that built the entry says
    /// so with [`EntrySurfaces::published`].
    pub from_published: bool,
    /// Whether the session's person, on a page that showed them every
    /// address this sign-in sends to, carried out this one sign-in. It
    /// allows one sign-in from drafts, and no renewal. False unless the
    /// caller that took the person's act says so with
    /// [`EntrySurfaces::shown_to_person`].
    pub shown_to_person: bool,
    /// Every address the target declares for its surfaces in the
    /// environment: where a sign-in may send.
    pub destinations: SignInDestinations,
}

impl EntrySurfaces {
    /// What `plan` holds about the surfaces of `auth`, in `environment`. A
    /// surface index the plan does not hold is left out; an environment it
    /// does not hold leaves every address as the target wrote it.
    #[must_use]
    pub fn of(plan: &Plan, auth: &AuthPlan, environment: &Environment) -> Self {
        let at = plan.environment_named(environment);
        let address_of = |index: usize, written: &str| {
            at.and_then(|at| {
                at.addresses
                    .iter()
                    .find(|address| address.surface.get() == index)
            })
            .map_or_else(|| written.to_owned(), |moved| moved.address.clone())
        };
        let mut surfaces = Vec::new();
        for index in &auth.surfaces {
            let Some(found) = plan.surface(*index) else {
                continue;
            };
            let mut surface = found.clone();
            surface.address = address_of(index.get(), &found.address);
            surfaces.push(surface);
        }
        let sections = plan
            .packs
            .iter()
            .filter(|section| {
                surfaces
                    .iter()
                    .any(|surface| surface.kind == section.surface_kind)
            })
            .cloned()
            .collect();
        let destinations = SignInDestinations::new(
            plan.surfaces
                .iter()
                .enumerate()
                .map(|(index, surface)| address_of(index, &surface.address)),
        );
        Self {
            surfaces,
            sections,
            from_published: false,
            shown_to_person: false,
            destinations,
        }
    }

    /// The same, marked as taken from a published version's plan. Only the
    /// caller that loaded the plan from the version store says so.
    #[must_use]
    pub fn published(mut self) -> Self {
        self.from_published = true;
        self
    }

    /// The same, marked as carried out by the session's person on a page
    /// that showed them every address the sign-in sends to. Only the caller
    /// that took the person's act says so.
    #[must_use]
    pub fn shown_to_person(mut self) -> Self {
        self.shown_to_person = true;
        self
    }

    /// Whether the entry was given nothing of its surfaces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::SignInDestinations;

    #[test]
    fn an_address_is_allowed_by_the_origin_of_a_declared_one() {
        let declared = SignInDestinations::new([
            "https://id.example.test".to_owned(),
            "http://127.0.0.1:8080/api".to_owned(),
            "postgres://db.example.test/orders".to_owned(),
        ]);
        for good in [
            "https://id.example.test/oauth/token",
            "https://id.example.test:443/token",
            "https://ID.example.test/token",
            "http://127.0.0.1:8080/other/path",
            "postgres://db.example.test/orders",
        ] {
            assert!(declared.allows(good), "{good}");
        }
        for bad in [
            "https://id.example.test.elsewhere.test/token",
            "https://elsewhere.example.test/token",
            "http://id.example.test/token",
            "https://id.example.test:8443/token",
            "http://127.0.0.1:8081/token",
            "http://localhost:8080/token",
            "https://user@id.example.test/token",
            "postgres://db.example.test/other",
            "not an address",
        ] {
            assert!(!declared.allows(bad), "{bad}");
        }
        assert!(!SignInDestinations::default().allows("https://id.example.test/token"));
    }
}
