//! A binding's two places (roadmap package 4.6, decision 0337).
//!
//! The compiler orders an operation's bindings by the rank of each binding's
//! mode, lower first, unless the operation says `binding_order: listed`. So
//! an operation's bindings have two places that differ once ordering moves
//! one: the **file's place**, which counts the bindings as the operation's
//! file lists them, and the **plan's place**, which counts them as the plan
//! holds them. A person reads and writes the file's place (a `position` in
//! a call, a `binding:` entry of a test file, a file rewritten by
//! `binding.promote`); the engine, a pin, and a run use the plan's place.
//!
//! The compiler orders bindings of equal rank by the file's order, and a
//! mode has one rank, so the bindings of one mode keep the file's order
//! among themselves. The k-th binding of a mode in the file is therefore the
//! k-th binding of that mode in the plan, whatever the order of the modes.
//! That is the whole conversion; it needs only the modes in each order.

use a2m_files::Sources;

/// The modes of the bindings of the operation named `tool`, in the order its
/// file lists them. `None` where no file defines the operation.
pub(crate) fn file_modes(sources: &Sources, tool: &str) -> Option<Vec<String>> {
    let doc = sources
        .of_kind("operation")
        .flat_map(|file| file.docs.iter())
        .find(|doc| doc.text("operation") == Some(tool))?;
    Some(
        doc.get("bindings")
            .map(a2m_files::Located::items)
            .unwrap_or_default()
            .iter()
            .map(|binding| binding.text("mode").unwrap_or_default().to_owned())
            .collect(),
    )
}

/// As [`file_modes`], reading the target's files in `folder` afresh. `None`
/// where they do not load or hold no such operation.
pub(crate) fn file_modes_in(
    kernel: &crate::kernel::Kernel,
    folder: &std::path::Path,
    tool: &str,
) -> Option<Vec<String>> {
    let set = kernel.providers().compiler_set();
    let sources = a2m_files::load(&a2m_files::TargetRoot::new(folder), set).ok()?;
    file_modes(&sources, tool)
}

/// How many bindings of the mode of `modes[at]` come before `at`.
fn nth_of_mode<M: AsRef<str>>(modes: &[M], at: usize) -> Option<usize> {
    let mode = modes.get(at)?.as_ref();
    Some(
        modes[..at]
            .iter()
            .filter(|other| other.as_ref() == mode)
            .count(),
    )
}

/// The place in `to` of the `nth` binding of `mode`.
fn place_of_nth<M: AsRef<str>>(to: &[M], mode: &str, nth: usize) -> Option<usize> {
    to.iter()
        .enumerate()
        .filter(|(_, other)| other.as_ref() == mode)
        .nth(nth)
        .map(|(at, _)| at)
}

/// The file's place of the binding the plan holds at `plan_place`, given
/// both orders of modes. `None` where the two do not hold the same number of
/// bindings, which is no conversion to guess at.
pub(crate) fn file_place<P: AsRef<str>, F: AsRef<str>>(
    plan_modes: &[P],
    file_modes: &[F],
    plan_place: usize,
) -> Option<usize> {
    if plan_modes.len() != file_modes.len() {
        return None;
    }
    let nth = nth_of_mode(plan_modes, plan_place)?;
    place_of_nth(file_modes, plan_modes[plan_place].as_ref(), nth)
}

/// The plan's place of the binding the file lists at `file_place`.
pub(crate) fn plan_place<P: AsRef<str>, F: AsRef<str>>(
    plan_modes: &[P],
    file_modes: &[F],
    file_place: usize,
) -> Option<usize> {
    if plan_modes.len() != file_modes.len() {
        return None;
    }
    let nth = nth_of_mode(file_modes, file_place)?;
    place_of_nth(plan_modes, file_modes[file_place].as_ref(), nth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_convert_both_ways_when_ranks_reorder_the_modes() {
        // The file lists the slow mode first, then two of the fast one; the
        // compiler puts the fast mode first, keeping the file's order within
        // a mode.
        let file = ["interface", "request", "request"];
        let plan = ["request", "request", "interface"];
        assert_eq!(file_place(&plan, &file, 0), Some(1));
        assert_eq!(file_place(&plan, &file, 1), Some(2));
        assert_eq!(file_place(&plan, &file, 2), Some(0));
        for at in 0..3 {
            let there = file_place(&plan, &file, at).unwrap();
            assert_eq!(plan_place(&plan, &file, there), Some(at));
        }
    }

    #[test]
    fn the_same_order_is_the_identity_and_unequal_lists_convert_nothing() {
        let same = ["a", "b", "a"];
        assert_eq!(file_place(&same, &same, 2), Some(2));
        assert_eq!(plan_place(&same, &same, 1), Some(1));
        assert_eq!(file_place(&["a"], &["a", "b"], 0), None);
        assert_eq!(plan_place(&same, &same, 3), None);
    }
}
