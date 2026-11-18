//! What a target's plan says about the surfaces one auth entry signs in to,
//! for a provider that reaches a surface of the system itself.
//!
//! A provider that only holds or exchanges a credential needs none of it. A
//! provider a pack registers because signing in needs the pack's own
//! surface, such as a sign-in through an application's own pages, needs to
//! know where the surface is and what the pack compiled about it: its
//! address in the session's environment, and the pack's sections of the
//! plan. They are plain data from the same plan a run uses, so the sign-in a
//! person makes and the session a run leases are one (decision 0245).

use a2m_core::{AuthPlan, Environment, PackSection, Plan, SurfacePlan};

/// The surfaces an auth entry covers, as the plan holds them for one
/// environment, and the plan's pack sections for their kinds. It holds no
/// credential: the plan never does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntrySurfaces {
    /// Each surface the entry covers, in the entry's order, at the address
    /// it has in the environment.
    pub surfaces: Vec<SurfacePlan>,
    /// The plan's pack sections registered for the kinds of those surfaces,
    /// in the plan's order.
    pub sections: Vec<PackSection>,
}

impl EntrySurfaces {
    /// What `plan` holds about the surfaces of `auth`, in `environment`. A
    /// surface index the plan does not hold is left out; an environment it
    /// does not hold leaves every address as the target wrote it.
    #[must_use]
    pub fn of(plan: &Plan, auth: &AuthPlan, environment: &Environment) -> Self {
        let at = plan.environment_named(environment);
        let mut surfaces = Vec::new();
        for index in &auth.surfaces {
            let Some(found) = plan.surface(*index) else {
                continue;
            };
            let mut surface = found.clone();
            if let Some(moved) = at.and_then(|at| {
                at.addresses
                    .iter()
                    .find(|address| address.surface == *index)
            }) {
                surface.address.clone_from(&moved.address);
            }
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
        Self { surfaces, sections }
    }

    /// Whether the entry was given nothing of its surfaces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }
}
