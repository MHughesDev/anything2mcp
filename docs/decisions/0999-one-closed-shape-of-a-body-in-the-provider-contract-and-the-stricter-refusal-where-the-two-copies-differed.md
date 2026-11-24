# 0999. One closed shape of a body in the provider contract, and the stricter refusal where the two copies differed

**Status:** Accepted. Design decision, made by the worker who built package E7a, under the lead's brief. The owner had not read this record when it was merged. It changes a public contract, the provider contract, by addition.

**Date:** 2026-11-24

**Changes:** [`a2m-provider`](../architecture/crates/a2m-provider.md), which gains the public module `shape`; [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md), whose `src/shape.rs` is removed; [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md), whose private `Kind`, `KINDS`, `Shape`, and `shape_schema` in `src/requests.rs` are removed. It builds point 1 of [decision 0394](0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), and corrects that point's sentence "no behaviour of either pack changes" by the lines under "Where the two copies differed".

## Context

The closed shape of a body was written twice: in full in the HTTP pack (`crates/a2m-pack-http/src/shape.rs`, the seven words and a recursive check over the pack's own JSON reader), and in part in the web pack (`crates/a2m-pack-web-ui/src/requests.rs`: a `JsonSchema` for a flat object, a `Kind` of seven words without `any`, and a check of each top-level value over `serde_json`). The browser base must check a page's body against the same shape (package E7b) and can depend on neither pack. Read side by side, the two copies did not refuse the same things. The lead's brief for E7a settled how: where they differ, the stricter refusal holds in both, each difference is written here as a line, and each has a test.

## Decision

1. **The module `shape` of `a2m-provider` is the one shape.** `Kind`, `Shape`, and `ShapeProblem` are public, with `Shape::of`, `Shape::from_doc`, `Shape::at_path`, and `Shape::check`. The words are the seven: `type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, `enum`.
2. **A value is checked through a view, and a misfit holds no value.** `Shape::check` takes any `ShapeValue`, which shows a value one level at a time as a `ValueView`; a number is shown as JSON writes it. The module implements it for `serde_json::Value`; the HTTP pack implements it for its own JSON, which keeps a number's text, so no value of that pack passes through a float. A failed check is a `Misfit`: a path of names and positions, and one of four kinds (another kind of value, a text outside the `enum`, a required name missing, a name a closed shape does not list). Each pack words it as it did before, so no failure a person or a test reads has changed its sentence.
3. **A pack may take fewer of the seven words, never more.** The HTTP pack takes all of them. The web pack's page request takes, in this version, `type: object` with `properties`, `required`, and `additionalProperties`, each property stating only a `type` other than `any`. What the shared reading accepts and the page request does not is refused by the web pack, under its own `request_shape_invalid`, at the word or the value. This is not a difference between the copies, but the part of the shape the page request is built for; package E7c widens it where `read_asserted` needs an `enum`.

## Where the two copies differed

Each line is one difference; the stricter refusal now holds for both packs.

- **An integer past 64 bits, and `-0`.** The HTTP pack took any whole number written without a fraction or an exponent as an `integer`; the web pack took only one `serde_json` reads as a 64-bit signed integer, which refuses a number past that range and `-0`, which it reads as a float. Now an integer has no fraction, no exponent, is not `-0`, and is within 64 bits, for either reader. Tests: `an_integer_is_one_that_fits_in_sixty_four_bits_with_no_fraction` (`a2m-provider`, `src/shape.rs`), and `an_integer_past_sixty_four_bits_or_negative_zero_does_not_fit_the_shared_shape` (`a2m-pack-http`, `src/json.rs`), through the HTTP pack's own reader.
- **A word whose value has the wrong form.** The HTTP pack read `properties` that is not a map, and `required` or `enum` that is not a list, as if nothing were written under the word, so a shape that looked closed checked nothing; the web pack's schema refused `properties` and `required` of the wrong form. Now each is refused where the value is written. Test: `a_word_whose_value_is_of_the_wrong_form_is_refused_at_the_value`.
- **A required name the shape does not list.** Both refused it; the HTTP pack at the shape, the web pack at the name. It is refused at the name. Test: `a_required_name_not_listed_is_refused_at_the_name`.
- **Where a page request's refusals are written.** The web pack's own schema refused a word a page request does not take as `kernel.unknown_key`, at validation. It is now refused by the web pack as `web.request_shape_invalid`, at the word or the value, after the shared reading. What it accepts is the same. Test: `a_page_requests_shape_takes_only_flat_typed_properties_of_the_shared_shape` (`a2m-pack-web-ui`, `tests/compile.rs`).

## Consequences

- The web pack's compiled `page_request` payload is unchanged: its values are of the shared `Kind`, whose words are the same, and the answer's `Shape` is made from them when it is checked. A compiled target made before this reads as it did.
- The HTTP pack's design said "packs share data shapes, never code". The shape is not shared between the packs: it is the contract's, as the manifest and `CheckedPath` are.
- The browser base can now check a page's body against the same shape, with `serde_json` and no pack (package E7b).

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep each pack's refusals where they differed, and choose nothing | One shape that refuses different things for different packs is two shapes behind one name |
| The looser refusal where they differed | A shape that looks closed and checks nothing is the failure the closed words exist to prevent; and an integer past 64 bits has no value type to land in |
| Move the HTTP pack's JSON reader into the contract, and check that one type | The web pack and the browser base read with `serde_json`; a view lets each keep its reader, and the HTTP pack keeps its rule that no number becomes a float |
| Make the web pack take the whole shape now | It would change what a page request accepts, which E7a must not do; E7c widens it with the key that needs it |

## Revisit if

A system answers integers past 64 bits that a tool must read as such; or the web pack takes the whole shape, which removes point 3.
