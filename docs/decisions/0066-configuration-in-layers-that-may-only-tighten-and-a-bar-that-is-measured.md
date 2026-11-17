# 0066. Configuration in layers that may only tighten, and a bar that is measured

**Status:** Accepted. The owner asked for the product to be highly configurable and to be the best of its kind; how was left to the building agent. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Refines:** the settings design of the first specification, now [section 226](../spec/2-kernel/226-configuration.md). **Changes a public contract:** the provider manifest gains a settings schema.

## Context

The first design had seven layers of settings and a short list of groups. The owner wants many ways to configure the product. More configuration is more ways to make a product unsafe, and this product's worth is that it is safe. The owner also wants it to be the best of its kind, which means nothing until it can be tested.

## Decision

**Configuration**

- **Everything has a default, and the defaults together are the safe single-user installation.** An empty file is a complete configuration. An unknown key is an error.
- **Eight layers,** lowest first: built-in defaults, the deployment profile, a preset, the installation's file, the user's file, the project's file, environment variables, a command's flags.
- **The upper layers may only tighten.** The user's file, the project's file, variables, and flags can make a protected or policy setting stricter and never looser. Each key declares which direction is safer. Some keys can be set only by the installation, and the installation can lock any key.
- **Configurability stops at safety.** Seventeen rules are guardrails: compiled in, with no key at all. They include no model at run time, no write without a recorded intent and a read-back, no secret to a caller, no writing during discovery, no approval by an agent, and no getting past a protection.
- **A protected setting is loosened only by a person,** through a channel an agent cannot reach. On one machine that is a presence prompt, and the value is kept in the store bound to that check. A file, a variable, or a flag that loosens one is refused.
- **Presets** are named bundles a person picks instead of setting twenty values: `standard`, `cautious`, and one for continuous integration, where no person is present and work that needs one stops at once. A preset may only tighten. A site can define its own. One at a time.
- **Twelve scopes,** from the installation down to a single run, each with where it lives and who may change it. A setting says how this installation behaves; a target file says what a system is.
- **Every pack brings its own group,** declared in its manifest with a schema, so the kernel validates it without knowing it. A pack's settings are ceilings or a choice of how; they never widen what a target declares.
- **A ceiling is checked when a version is compiled and again when it is imported.** A version above the ceiling is refused, never trimmed.
- **Tightening takes effect at once; loosening only for runs that start after it.** The digest of the settings in force is recorded at every start.

**The bar**

- "Best" is twelve dimensions, each with what the best looks like, the test that shows it, and the roadmap package that measures it ([section 107](../spec/1-product/107-the-bar.md)).
- The bar includes what the product will not do to look better: improvise with a model, get past a protection, hide that it is automation, guess.
- A scorecard states each dimension as designed, built, or measured. Today nearly every row is designed. Every milestone ends by updating it, and a number that misses its target changes the design or the target, by decision record.

## Consequences

- The provider manifest gains a settings schema. The design of `a2m-provider` and the manifest example follow when package B3 builds the manifest.
- The settings loader is its own roadmap package, 1.4a.
- The earlier settings examples are archived; section 226 has its own.
- Several keys of the first design are gone because they could loosen a guardrail or name a mode: the order of modes is not a setting, and the approval of irreversible operations has no key.

## Alternatives considered

- Let any layer set any value, as most tools do: simple, and a project file checked out from elsewhere could lower an installation's approval rules.
- Few settings and strong opinions: safest, and it would not fit the range of systems and deployments the product is for.
- Declare the product the best on the strength of its design: free, and worth nothing.

## Revisit if

The tighten-only rule blocks something legitimate that cannot be done through a protected setting, or a scorecard row cannot be measured.

## Spec sections

- [226-configuration.md](../spec/2-kernel/226-configuration.md)
- [107-the-bar.md](../spec/1-product/107-the-bar.md)
