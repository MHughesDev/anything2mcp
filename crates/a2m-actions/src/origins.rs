//! The addresses a person registered for a target (spec section 502, rule
//! 2; decision 0413): the address given to `target.add`, and every address
//! of every version a person published. A draft that reaches another is
//! published only from the publish page, which names it.

use std::collections::BTreeSet;

use a2m_core::{CompiledTarget, Failure, TargetName};
use a2m_ledger::Durability;

use crate::kernel::{Kernel, fact, set_fact};

/// Where a surface's address leads, to be compared: for an address with a
/// scheme and a host, its scheme and authority, lower-cased, with no path,
/// so that another path at the same place is the same; for any other
/// address, such as a folder or a program, the address whole.
pub(crate) fn origin_of(address: &str) -> String {
    let trimmed = address.trim();
    match trimmed.split_once("://") {
        Some((scheme, rest)) if !scheme.is_empty() && !rest.is_empty() => {
            let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
            // What precedes an `@` names a user, not the place.
            let place = authority.rsplit('@').next().unwrap_or_default();
            format!(
                "{}://{}",
                scheme.to_ascii_lowercase(),
                place.to_ascii_lowercase()
            )
        }
        _ => trimmed.to_owned(),
    }
}

/// Every place the compiled target's surfaces lead to, in every
/// environment it declares.
pub(crate) fn origins_of(compiled_target: &CompiledTarget) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (index, surface) in compiled_target.surfaces.iter().enumerate() {
        found.insert(origin_of(&surface.address));
        for environment in &compiled_target.environments {
            for given in &environment.addresses {
                if given.surface.get() == index {
                    found.insert(origin_of(&given.address));
                }
            }
        }
    }
    found
}

fn fact_name(target: &TargetName) -> String {
    format!("registered_origins:{target}")
}

impl Kernel {
    /// The places registered for `target`; empty when none is.
    pub(crate) async fn registered_origins(
        &self,
        target: &TargetName,
    ) -> Result<BTreeSet<String>, Failure> {
        let name = fact_name(target);
        let text = self.store()?.read(move |tx| fact(tx, &name)).await?;
        Ok(text
            .and_then(|text| serde_json::from_str::<BTreeSet<String>>(&text).ok())
            .unwrap_or_default())
    }

    /// Adds `places` to those registered for `target`.
    pub(crate) async fn register_origins(
        &self,
        target: &TargetName,
        places: BTreeSet<String>,
    ) -> Result<(), Failure> {
        let name = fact_name(target);
        self.store()?
            .write(Durability::Synced, move |tx| {
                let mut held = fact(tx, &name)?
                    .and_then(|text| serde_json::from_str::<BTreeSet<String>>(&text).ok())
                    .unwrap_or_default();
                held.extend(places);
                let text = serde_json::to_string(&held).map_err(|_| {
                    crate::codes::busy(
                        a2m_ledger::codes::STORE_UNAVAILABLE,
                        "The registered addresses could not be written down.",
                        "Report this; it is a fault in the engine.",
                    )
                })?;
                set_fact(tx, &name, &text)
            })
            .await
    }

    /// The places `compiled_target` leads to that no person registered for
    /// `target`. Empty when none is registered yet: a target written by
    /// hand registers its places with the first version a person
    /// publishes, whose page shows every one of them.
    pub(crate) async fn unregistered_origins(
        &self,
        target: &TargetName,
        compiled_target: &CompiledTarget,
    ) -> Result<Vec<String>, Failure> {
        let registered = self.registered_origins(target).await?;
        if registered.is_empty() {
            return Ok(Vec::new());
        }
        Ok(origins_of(compiled_target)
            .into_iter()
            .filter(|place| !registered.contains(place))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn a_place_is_its_scheme_and_authority_and_anything_else_is_whole() {
        assert_eq!(origin_of("http://127.0.0.1:4010"), "http://127.0.0.1:4010");
        assert_eq!(
            origin_of("HTTP://Example.Invalid:8080/api/v1?x=1"),
            "http://example.invalid:8080"
        );
        assert_eq!(
            origin_of("https://user@example.invalid/path"),
            "https://example.invalid"
        );
        assert_ne!(
            origin_of("http://127.0.0.1:4010"),
            origin_of("http://127.0.0.1:9")
        );
        assert_eq!(origin_of("/srv/files/orders"), "/srv/files/orders");
    }
}
