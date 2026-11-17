# 66. Readiness of the universal design

Sections 53 to 65 widen the design from applications to any existing system. This section says how much of that is design a builder can work from, how much is a catalog, and what must be proven before any of it is leaned on. The [readiness report](../plan/readiness.md) covers sections 1 to 52; the [long-term plan](../plan/anything2mcp-master-plan.md) gives the order of work.

## State of each section

| Section | State | A builder can | A builder must not |
| --- | --- | --- | --- |
| 53. The universal runtime | A statement of scope | Use it to decide whether something belongs | Treat a level of the stack as supported because it is named |
| 54. Interaction archetypes | Designed in outline | Classify a new mode; see what its driver must supply | Build a mode whose steps are still "to be fixed with its first pack" without fixing them first |
| 55. Kernel and packs | Designed | Draw the crate boundary and enforce it; apply the entry bar | Let a pack reach the kernel outside the six registrations |
| 56. The provider contract | Designed, without its file format | Write the manifest's schema from the field list | Add an extension point |
| 57. Supervising subsystems | Designed as rules | Build the supervisor against the rules | Promise confinement of plug-ins before the research is done |
| 58. Discovery and findings | Designed, without its closed lists | Build the findings store and the comparison | Let anything raise a finding above `discovered` |
| 59. The evidence ladder | Designed | Add the level to core types; tie it to the compiler | Add a field that sets a level by hand |
| 60. Overlaying tools | Designed as a method | Follow it for the web pack now | Write beneath a system's own rules without its owner's word |
| 61. Effects, hazards, consent | Designed | Add hazards to operation files and policy | Claim `confined` anywhere yet |
| 62. Catalog of surface kinds | A catalog | Pick the next pack | Start a pack from a row in a table |
| 63. Devices and firmware | Rules and a catalog | Know what will be refused | Build a device pack without designing it as a section first |
| 64. Rules for tools | Designed | Apply them to every action from now on | Add an action that mixes discovery with change |
| 65. Python-first packaging | Designed in outline | Plan the wheel | Count a name as held before a release is published under it |
| 67. The scout, and who does what | Designed, without each pack's drafting rules | Build the pipeline's stages as the actions they are made from come to exist | Let the scout try a write, or draft a unit, key, or read-back rule the system did not state |

## What rests on judgment alone

Every one of these sections. None has met code. The parts taken from REA have met code in REA, in a different language, for a different purpose, which is evidence that they can work and not that they work here.

## What must be proven, and where

| Bet | Hypothesis | Proven or refuted by |
| --- | --- | --- |
| The kernel can stay neutral | H-26 | Adding the command pack: Stage D of the long-term plan |
| An outside discovery provider is worth its cost | H-27 | Stage E |
| One binary with every first-party pack stays small | H-28 | Stage F |
| Seven archetypes are enough | H-29 | Each pack's design, recorded as it is made |
| Reading back through a second surface catches more | H-30 | Phase 3, in the test application |
| The crate layering holds | H-25 | Every slice, through Phase 3 |

## What is deliberately not designed yet

| Part | When |
| --- | --- |
| The manifest's file format, and how a plug-in registers schemas across a process boundary | With core types: package B3 of the long-term plan |
| The steps of each new archetype | With its first pack |
| Confinement of plug-in processes on each operating system | Before any plug-in may claim more than `declared` |
| Every pack beyond the first four and the command pack | One at a time, each as its own section |
| A device pack of any kind | After the command pack has tested the boundary, and with a named need |
| Runs started by an event | With triggers, which were already deferred |
| The scenario file format | With the first discoverer |

## The order, in one paragraph

Rename and separate first, because it is mechanical and cheap now. Then the kernel's contracts, as types and schemas. Then the core, through the web pack, exactly as the core-first slices already order it. Then one pack as unlike the web as possible, to test the boundary. Then discovery. Then packaging. Everything else follows one pack at a time. The long-term plan has this as stages A to F with work packages.

## What does not change with any of this

The first usable product is one machine, one person, reads and writes to an application through the web pack, with every write approved, verified, and resumable. Nothing in sections 53 to 65 is needed to ship that, and nothing in them may delay it.
