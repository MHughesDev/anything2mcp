# 0341. Endpoint inference as built: the counts, the short number, the sibling words, and the map

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out points 1, 4, 5, 6, and 7 of [decision 0322](0322-a-capture-never-infers-required-and-types-a-parameter-from-every-value.md), whose points stand; it states the exact rules the web probe now holds, where that decision left a figure or a test open.

**Date:** 2026-11-24

**Changes:** the shapes, tier one, tier two, and the "Dependencies" row of endpoint inference in [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md); what a finding keeps; spec section 301 where it describes inference.

## Context

Decision 0322 settled what endpoint inference must not claim, and left the rules to the package that builds them. Building them needed four choices that the decision did not make.

## Decision

1. **A field seen in every answer carries `seen_in_all`, with a count, once at least two objects were seen at its place.** The count is of the objects of that place the shape was merged from (answers, and the items of a list, which the shape already counts for `optional`). One object seen says nothing about "every", so it carries no mark. `required` is never written. A field missing from some is `optional`, as before.
2. **A number in a path is a parameter by its form only at four digits or more.** A shorter number stays literal, and the endpoint says so in a limitation of its own (`sampled`). Tier two, when asked, makes it a parameter where requests showed it varying. Four digits is the starting value of decision 0322, kept as one named constant for the first test to tune; it is not measured. Dates, universally unique identifiers, and long hexadecimal strings are not affected.
3. **Two words are not one parameter when the capture uses them as fixed names.** Tier two does not join two endpoints that differ in one segment when both values are plain words (letters first, no digit-only text) and each is also a literal at that place in another endpoint of the capture: one with the same origin, the same segments before the place, and a path of another length or another method. Words with no such use elsewhere are joined, and the finding says how many values were seen (`values_seen`; two is the least evidence). This is the cheapest test that the capture itself shows the words are names; it is not a model of meaning, and a capture too small to show the other use joins two words that are names.
4. **An object whose keys are all identifiers by form is a `map`, whatever its size.** The identifiers are numbers of any length, universally unique identifiers, long hexadecimal strings, and dates. Its keys are never named; the shape of its values is merged and kept as `items`. The older test, more than 64 names or a name that is not a plain name, still withholds the names of any other object.
5. **Dependencies are found by the digest comparison and nothing else.** A request depends on an earlier answer only where the digest of one value is equal in both. No model is used, and no other evidence is taken (rule 1; the method is written in the "Dependencies" row of the design). The finding is not built; this point fixes its method.

## Alternatives

| Alternative | Why not |
| --- | --- |
| `seen_in_all` on a single object | One sample is not "every"; the mark would be on every field of every capture and say nothing |
| A list of known words to keep literal | It names a language and a kind of software; the capture's own other paths are evidence that needs neither |
| No length floor on numbers, left to tier two | A page number such as `2` would become a parameter from one request |

## Proof

In `crates/a2m-pack-web-probe/src/tests/rules.rs`: `a_field_seen_in_every_answer_is_counted_and_never_required`, `a_short_number_in_a_path_stays_literal_unless_it_varies_or_has_four_digits`, `two_words_the_capture_uses_as_fixed_names_are_not_one_parameter`, and `an_object_keyed_by_identifiers_is_one_map_whatever_its_size`. The counts of values seen are held by the second.

## Not done

The drafter in `a2m-pack-web-ui` (`draft_request.rs`) still drafts an output as `required` when its field is not `optional`. That is the drafter's reading of a finding, and decision 0322 point 3 (the held draft asks which parameters may be left out) is not built there.
