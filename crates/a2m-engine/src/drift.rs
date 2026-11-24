//! Which knowledge a drift was seen in, and which bindings of a compiled
//! target it stops (decision 0380).
//!
//! A canary, or any run, that fails with `drift` at a step names the pack
//! entry the step refers to: in the web pack a screen, in another pack the
//! entry of its kind. That entry is the knowledge that no longer fits the
//! system, and every binding of the compiled target with a step that refers
//! to the same entry, through the same mode on the same surface, is stopped
//! by it: a write that would go through a screen a canary found changed is
//! refused before anything is sent. A drift no step named stops every binding
//! of its mode on its surface, since nothing says which knowledge it was in.
//!
//! A mark is held to the knowledge, not to the version: an entry by the
//! digest of the whole pack section that holds it, a binding by its own
//! digest. So a new version that changes the drifted knowledge, a repair,
//! is not stopped by it, and a new version that leaves it as it was still
//! is. The engine knows nothing of what an entry means; it compares names
//! and digests.

use a2m_core::{
    CanonicalJson, CompiledBinding, CompiledTarget, Digest, Mode, OperationName, PackRef, Step,
    SurfaceName,
};

use crate::binding::binding_digest;

/// The knowledge a drift was seen in, or a canary's pass vouched for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DriftKnowledge {
    /// One entry of a pack section: the digest of the section as the compiled
    /// target holds it ([`section_digest`]), and the entry's name in it.
    Entry {
        /// The digest of the whole section.
        section: Digest,
        /// The entry's name, as its pack defines names.
        entry: String,
    },
    /// A binding whose drift no step named: its digest
    /// ([`binding_digest`]).
    Binding(Digest),
}

/// A drift a canary or a run found, as the control plane keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftMark {
    /// The operation whose binding drifted.
    pub operation: OperationName,
    /// The binding's mode.
    pub mode: Mode,
    /// The surface it acts on, by name.
    pub surface: SurfaceName,
    /// The knowledge that drifted.
    pub knowledge: DriftKnowledge,
}

/// The digest of one pack section of `compiled_target`: what it holds, its
/// pack, its kind, and its format. `None` when the compiled target holds no
/// such section or it cannot be encoded.
#[must_use]
pub fn section_digest(compiled_target: &CompiledTarget, reference: &PackRef) -> Option<Digest> {
    let section = compiled_target.packs.get(reference.section.get())?;
    CanonicalJson::encode(section)
        .ok()
        .map(|encoded| encoded.digest())
}

fn steps_of(binding: &CompiledBinding) -> impl Iterator<Item = &Step> {
    binding
        .act
        .iter()
        .chain(&binding.confirm)
        .chain(&binding.read_back)
        .chain(&binding.lookup)
}

/// The knowledge a drift at `step` of `binding` is in: the entry the step
/// refers to, or, when it refers to none or no step is known, the binding
/// itself. `None` when the compiled target does not hold what either names.
#[must_use]
pub fn drifted_knowledge(
    compiled_target: &CompiledTarget,
    binding: &CompiledBinding,
    step: Option<&Step>,
) -> Option<DriftKnowledge> {
    match step.and_then(|step| step.refers_to.as_ref()) {
        Some(reference) => Some(DriftKnowledge::Entry {
            section: section_digest(compiled_target, reference)?,
            entry: reference.entry.clone(),
        }),
        None => binding_digest(compiled_target, binding).map(DriftKnowledge::Binding),
    }
}

/// Every piece of knowledge a pass through `binding` relies on: each entry
/// its steps refer to, and the binding itself. What a canary that passed
/// through it vouches for again.
#[must_use]
pub fn knowledge_of(
    compiled_target: &CompiledTarget,
    binding: &CompiledBinding,
) -> Vec<DriftKnowledge> {
    let mut known: Vec<DriftKnowledge> = steps_of(binding)
        .filter_map(|step| step.refers_to.as_ref())
        .filter_map(|reference| {
            Some(DriftKnowledge::Entry {
                section: section_digest(compiled_target, reference)?,
                entry: reference.entry.clone(),
            })
        })
        .collect();
    if let Some(digest) = binding_digest(compiled_target, binding) {
        known.push(DriftKnowledge::Binding(digest));
    }
    known.sort();
    known.dedup();
    known
}

/// Every binding of `compiled_target`, by its operation and mode, that one of
/// `marks` stops: a binding through the mark's mode, on the mark's surface,
/// with a step that refers to the entry the mark names, in a section with the
/// digest the mark holds; or, for a mark of a whole binding, every binding
/// through its mode on its surface, while the compiled target still holds a
/// binding of the mark's operation with that digest. Each pair once, in
/// compiled target order.
#[must_use]
pub fn drift_covers(
    compiled_target: &CompiledTarget,
    marks: &[DriftMark],
) -> Vec<(OperationName, Mode)> {
    let mut covered = Vec::new();
    for operation in &compiled_target.operations {
        for binding in &operation.bindings {
            let Some(surface) = compiled_target.surface(binding.surface) else {
                continue;
            };
            let stopped = marks.iter().any(|mark| {
                if mark.mode != binding.mode || mark.surface != surface.name {
                    return false;
                }
                match &mark.knowledge {
                    DriftKnowledge::Entry { section, entry } => steps_of(binding).any(|step| {
                        step.refers_to.as_ref().is_some_and(|reference| {
                            &reference.entry == entry
                                && section_digest(compiled_target, reference).as_ref()
                                    == Some(section)
                        })
                    }),
                    DriftKnowledge::Binding(digest) => compiled_target
                        .operations
                        .iter()
                        .filter(|other| other.name == mark.operation)
                        .flat_map(|other| &other.bindings)
                        .any(|other| {
                            other.mode == mark.mode
                                && binding_digest(compiled_target, other).as_ref() == Some(digest)
                        }),
                }
            });
            let pair = (operation.name.clone(), binding.mode.clone());
            if stopped && !covered.contains(&pair) {
                covered.push(pair);
            }
        }
    }
    covered
}
