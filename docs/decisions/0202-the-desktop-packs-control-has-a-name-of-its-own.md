# 0202. The desktop pack's control has a name of its own

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the control the desktop pack registers, named `text` in [decision 0198](0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md), the [desktop pack's design](../architecture/crates/a2m-pack-desktop.md), and [spec section 305](../spec/3-packs/305-the-desktop-pack.md). Nothing else in 0198 changes.

## Context

Control names are one registered namespace across packs. The web UI pack registers a control `text`, and the desktop pack registered `text` as well. With both packs in one registry, as they are in the product, the registry refused the second with `registration_conflict` ("The control `text` is registered by `desktop` and again by `web`"), and the web pack was not registered at all. Four product tests failed.

## Decision

The desktop pack's control is named `desktop_text`. Changed: the `CONTROLS` constant and the manifest entry, the drafting rules that write the control into a drafted screen, the fixtures and the docs that name it. A desktop screen file says `control: desktop_text`.

Left as they were, because they are other meanings of the word: the accessibility role `text` and the locators that use it, and the `text` wait condition (equals or contains).

## Alternatives considered

- **Let two packs register one control name, each for its own screen kind.** It would change the kernel's registry so that a name means two things, against the rule of one vocabulary. Not taken.

## Consequence

A screen file written for the desktop pack before this record, with `control: text`, is refused by the compiler as an unknown control and must say `control: desktop_text`. No such file exists outside the pack's fixtures. The other desktop controls the spec lists (`textarea`, `number`, and so on) will be registered with the prefix `desktop_` when built, if the web pack has taken the same name.
