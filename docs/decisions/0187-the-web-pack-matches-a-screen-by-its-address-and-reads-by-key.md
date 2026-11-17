# 0187. The web pack matches a screen by its address, reads by key, and reads labelled values

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the drafting rules and the driver of [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md), the `web_snapshot` discoverer of [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md), the in-page helper of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), additions to the test application's web interface, and sentences of spec sections [301](../spec/3-packs/301-the-web-pack.md) and [402](../spec/4-building-tools/402-the-scout-and-who-does-what.md). Takes up what [decision 0183](0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md) left: screen matching and keys, and labelled values.

## The plan, as written before building

1. A screen is matched, not only reached by one path. The driver already fills a placeholder of `reach` from an input; it now refuses, before any navigation, a value that is not one plain segment, instead of percent-encoding it. The discoverer joins the snapshots of pages whose paths differ in one segment and that show one screen into one `screen` finding whose subject is the address with a placeholder, naming the pages it was seen on. The drafting rules read a placeholder in the subject's path as a key.
2. Labelled values. The probe keeps the name of a description list's term, and the name of a definition, which a page gives only by labelling it (the browser gives a definition no name from what it holds). The rules draft a `get` read whose outputs are the definitions a term of the same name labels.
3. A read over a request the page made: see the section on it below.

*Alternative weighed for 1:* screen matching in the drafter, over every finding at once. The drafter calls a rule with one finding (decision 0183), so it would need a rule that sees all of a draft's findings; the discoverer already sees every page of one discovery. *For 2:* locating the value as the element after its term, by a new locator kind. No locator kind says "beside", and the snapshot and the helper would have to agree on a rule the browser does not keep.

## Decision

| Question | Decided | Alternative weighed |
| --- | --- | --- |
| What a placeholder's value may be | One segment of letters, digits, `-`, `.`, `_`, and `~`, not made of dots only, at most 512 bytes; a whole number is written in digits. Anything else, a `/`, `\`, `?`, `#`, `%`, `:`, a space, or a control character among them, is `input`, `address_value_refused`, and nothing is sent. The made address is then checked to be the surface's origin and the screen's own path pattern, segment by segment, before the tab is sent | Percent-encoding every other character, as built before. A value encoded once can still become a separator in an application that decodes twice, and `%2e%2e` is a dot segment to some; no record of the test application needs a key outside the plain form. A key of another form (a space, a letter outside ASCII) cannot be reached by a `ui` read until a field type states how to write it |
| Which pages are one screen | Pages of one discovery whose paths have the same number of segments and differ in exactly one, and whose snapshots give the same screen: the same named tables with the same headings in the same parts of the page, and the same labelled values, at least one of either. The heading of the page, its links and paragraphs, and how many rows it shows are not compared | Comparing whole snapshots. Two orders with a different number of lines, or one that can still be changed, never match |
| What the discoverer gives for them | One `screen` finding, still of format 1, whose subject is the path with that segment as a placeholder, `found` the snapshot of the first page by path with `pages`, the paths it was seen on, added; the pages' own `screen` findings are not given. A page that would join two such groups joins neither. The placeholder is named from the segment before it, as one record of it (`orders` gives `order_id`), and `key` when no plain word comes before it | Giving the per-page findings as well. Each would draft its own screen beside the matched one, named for one record |
| What the rules draft from it | The screen with `reach` the pattern, and the read with the placeholder as its input, of type `text`, and as its `key`. Names are made from the path without its placeholders, so the read of the line items of an order is `{target}_orders_line_items_list` | Naming the read `..._by_order_id`. The key is in the read's inputs; the name says what is read |
| Labelled values | A term (a description list's term) keeps its name outside a record, and so does a definition, whose name a page gives only by labelling it. Each definition whose name one term of the page also has, once, inside a part of the page the snapshot names once, is drafted as a field of `screens/{page}_values.yaml`, located by the role `definition` and that name, read as text, `untrusted_text`, and an output of `{target}_{page}_get` (`verb: read`) | Locating the value by its term with a `within` or a position. Neither is a locator by meaning |
| Two terms of one name | No field and no output for either; the question is drafted apart, in `screens/{page}_values_undecided.yaml`, so that the read of the other values still compiles (as the MCP pack keeps an undecided mapping apart, decision 0179) | Putting the question in the read's screen. The whole read would then not compile for one ambiguous label |
| The test application | The page of one order labels each definition by its term (`aria-labelledby`), and a control route, `/__control/seen`, lists the method and path of every request the application received, so that a test can show that a refused value sent none | Adding a second order page for the labelled values. The task is the order page's |

## Consequences

To be written when built.
