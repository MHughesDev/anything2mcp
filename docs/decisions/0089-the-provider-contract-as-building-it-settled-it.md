# 0089. The provider contract as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the interface sketch of `a2m-provider` in [decision 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md), where the code differs. The driver, discoverer, and action interfaces and the manifest are public contracts, so each change is recorded here.

## Decision

1. **Boxed futures, no macro.** Every async method of `Driver`, `Discoverer`, `SandboxPort`, and `ProviderAction` returns `BoxFuture<'a, T>`, the same type `a2m-auth` uses, so each trait is a trait object and no attribute macro enters the tree. *Instead of:* the `async_trait` macro of the sketch, which is a proc-macro tree for the same boxing.
2. **A pack is handed a `PackContext` when it registers:** `Pack::register(&self, cx: &PackContext, to: &mut Registrar)`. The context carries a supervisor scoped to the manifest, the roots the manifest declares, the redactor, telemetry, and the pack's validated settings. The registry builder is given these once, as `HostServices`. *Instead of:* passing each to every driver call, which would leave an auth provider or a lowering with no way to reach them.
3. **Every `Registrar` method returns a result.** A registration beyond the manifest is `policy`, `effect_not_declared`, and the pack is listed with it and registers nothing.
4. **`CompilerRegistrations` is this crate's:** a plain list of the objects `a2m-files` defines. The registry feeds them to `a2m-files`'s builder in order of provider name, and a provider the builder refuses is left out whole.
5. **The driver's context.** `PrepareContext` carries the run, its lease number, the surface, the plan as data, the posture, a deadline, the redactor, the destinations, the checked paths, and the scoped supervisor. `StepInput` carries the evaluated arguments, the commit point (`RecordRef`), the record's key, its idempotency token, and a deadline. The handle compares a token's run and lease number with what `prepare` was given and its record with the step's input. `WidgetSet` is left out until the widget driver trait exists.
6. **A failure code is declared.** A driver's failure passes when its code is in the manifest's `failure_codes` with the same class, is the step's own `fails_as`, or is a code of this crate or of the session keeper. Anything else becomes `unknown_variant`, `undeclared_code`.
7. **A discoverer's findings are checked as they arrive,** by `DiscovererHandle::discover`, against the kinds and the level its manifest declares. `ReadOnlyPort` is a ready `SandboxPort` over one prepared driver; it passes every step with the right to read only.
8. **The manifest gains what the registry needs:** `[[finding_kinds]]`, `[[controls]]`, `[[failure_codes]]`, `surface_kinds` on a file kind and on a discoverer, and `[[settings]]`. A file kind's `schema` path is for plug-ins; a pack registers the schema as an object.
9. **A pack's settings schema is written in the manifest,** one `[[settings]]` entry per key with its type, default, range or choices, scope, whether it is protected and which direction is safer, and its description. *Instead of:* a JSON Schema file beside the manifest, which a compiled-in pack does not have a folder for and which would need a validator crate.
10. **The programs a pack may start** are those under `effects.processes` and the places its `[[requirements]]` are looked for. A requirement is a program the manifest names by fixed path, which is what the rule asks.
11. **Names.** `ProviderName` (a pack's or plug-in's; not the auth provider's name of `a2m-auth`), `RootName`, `SubsystemKind`, `DiscovererName`, and `ActionName` are checked names of this crate. The design's `SourceFormat` is `a2m-sources`'s `ReaderName`.
12. **Problems that do not stop a provider.** A kind a provider reads whose owner is missing is reported with `registered: true`. Every other problem leaves the provider out.

## Consequences

- The design of `a2m-provider` carries the interface as built.
- [Spec section 226](../spec/2-kernel/226-configuration.md) says where a pack's settings schema is written.
- The published plan has no neutral place that names the programs a command surface starts. Until it has one, the engine passes them to `DriverHandle::supervisor`.
