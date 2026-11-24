# 0399. The remaining widget drivers: built from what a page says of its controls, a staged file, and a grid cell by its row's key

**Status:** Accepted. Design decision. The owner had not read this record when it was written. Carries out package 8.4 for its widget drivers, and answers point 8 of [decision 0350](0350-the-phase-1-cases-of-the-interface-driver-frames-answered-questions-native-dialogs-two-more-controls-and-what-the-grid-still-needs.md). The platform profiles that the row of 8.4 also named are not built here; they are package 8.4a.

**Date:** 2026-11-24

**Changes:** the widget driver table in [spec section 301](../spec/3-packs/301-the-web-pack.md); the `screens/` format and the binding of the `ui` mode in [file formats](../reference/file-formats.md) (`options` for more controls, `column`, and `rows`); the controls the web pack registers; one failure code, `file_not_staged`; the designs of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md) and [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md); case 10 of the [test application's plan](../plan/test-application.md).

## Context

Package 8.4 adds the controls the web pack had no rule for: `combobox`, `autocomplete`, `datetime`, `time`, `slider`, `file`, `rich_text`, `grid`, `tree`, `tabs`, `accordion`, and `toggle`. A page draws each of these for itself, so there is no element the browser gives a value to. What the driver can trust is what the page says of the control's state, in the attributes that assistive software reads (`aria-selected`, `aria-expanded`, `aria-checked`, `aria-valuenow`), and what the control itself holds (a text box's value, a file input's files).

The alternative designs considered were to give each control a script of the engine's own to run in the page, which would put engine code in the page's world and make each rule depend on the page's framework, and to drive each control by the mouse at coordinates, which cannot tell a control that took a click from one that ignored it. The design below uses the base's looking, clicking, keys, and reads, and never a script of ours that changes the page.

## Decision

1. **Each of the twelve controls has a rule, and a rule sets and then reads the control to find it as meant.** A rule that sent input and does not find the control as it meant it stops with `unexpected_control` and the fate of the input unknown (it is not undone). A control name the driver has no rule for stays a stop, `unknown_control`, as before (rule 7).

   | Control | Set | Read |
   | --- | --- | --- |
   | `datetime`, `time`, `rich_text` | The text rule of `text`, in the form the browser's field takes (`2026-12-01T09:30`, `17:30`), or into an editable element | The field's value, or the editor's text |
   | `combobox`, `autocomplete` | Type the label, wait up to five seconds for the option of exactly that label, press it | The label shown, as its code through the field's `options`, or as it is when the field has none |
   | `slider` | Press Right or Left, one key at a time, reading after each, toward the value; at most 500 keys | The input's value, or `aria-valuenow` of a slider the page draws |
   | `file` | Give the input a file of the staging folder, by name | The names of the files the input holds |
   | `grid` | Press the cell of the row of a key and the column, type into the editor it opens, press Enter | The text of that cell of the row the page holds |
   | `tree` | For each label of the path but the last, press Right if the item says `aria-expanded="false"`; press Enter on the last | The labels of the item that says `aria-selected="true"` and of the items it is under |
   | `tabs`, `accordion` | Press the tab, or the button of the section, unless its attribute (`aria-selected`, `aria-expanded`) is already `true` | The code of the first choice whose attribute is `true`, through `options` |
   | `toggle` | As `checkbox`, for an element that says `aria-checked` | The same |

2. **A screen's `options` is no longer for a `radio` alone.** A `radio`, `tabs`, and `accordion` field must name its choices under `options`, each code with the label the page shows, since each is read as the code of the choice that is on. A `combobox` and an `autocomplete` may; with none, the label is the value. No other control takes `options`, and the compiler refuses it (`web.options_not_fitting`).

3. **A grid cell is a field with a `column`, and its row is an input of the binding.** A `grid` field names the `column` of its cell, its place among the cells of a row counted from zero, and no other control takes one (`web.column_not_fitting`). The binding says which row in `rows`, a map from the grid field's name to an expression for the key, which is the text the row's first cell shows. A `rows` entry that names a field that is not a grid cell is refused (`web.rows_not_fitting`), and a grid cell with no `rows` entry is refused (`web.row_missing`). The key is not part of an address and takes any text, so it is not held to a segment's characters. The step carries it as the argument `row.<field>`.

4. **A file is given by the name of a file of the staging folder, never by a path.** The staging folder is `staging/` inside the web pack's profile root, a checked path. A name that is not one plain name (a separator, `..`, a drive, empty), that names a folder, or that no file of the folder has, is refused before anything is sent, with the code `file_not_staged` of the class `input`. The base gives the file input its file through the DevTools command that sets the files of an input (`DOM.setFileInputFiles`), and only for an element that is a file input. A path from a caller is never opened, which is the rule of spec section 502: no input changes which file is read. What puts a file into the folder is not this record's: a pack that exports one, or a person, does.

5. **Point 8 of decision 0350, answered: a `ui` read of a grid cell reads the row the page holds, in the observing tab, and a row it does not hold is `locator_not_found`.** The row is found by its key (`By::RowKeyed`) and the cell by its place (`ReadWhat::Cell`), and the read does not scroll, filter, or type. A grid that has to be moved to show its row, such as the virtualized grid of case 9, is read through the network, as the widget table has always said ("the network response preferred"). A read in an acting tab was weighed and refused, as in decision 0350: a read that may type into a page is a read that may write by accident (decision 0195). The editing of a cell, which is a write, runs in the acting tab and finds its row the same way: a row the page does not hold is a stop, and a finder that brings a row in is not built.

6. **A tree's path is one text of labels joined by `/`.** A label that holds a `/` cannot be addressed and is a stop; an empty label is refused. The keys are those of the tree pattern for assistive software (Right opens, Enter selects); a tree that opens and selects only by a click, or that does not say `aria-expanded`, is a stop with a message that names the item.

7. **Four additions to the base.** `By::CellKeyed { key, column }` finds one cell of the row of a key. `ReadWhat::TreePath` and `ReadWhat::Files` read what the two controls above hold. `Input::Attach(path)` gives a file input one file. A switch, check box, or radio button that a page draws and marks with `aria-checked` is now one `Input::Check` and `ReadWhat::Checked` can set and read. No existing behaviour changes.

8. **The picker of a date is not built.** A date field that takes no typing (`readonly`) is a stop under the text rule, `unexpected_control`, with nothing sent; the spec's "fall back to the picker" is still a gap. So is the normalization of `datetime` and `time` to a time zone: the value is the one the field holds.

9. **Platform profiles are package 8.4a.** The row of 8.4 named `profiles/`; the data files, their loader, and the locator lint and input commit they feed are not built, and have no test stand beyond the one the pack has. They are a package of their own, needing 8.4, so that 8.4's proof, which is about the controls, is stated and met.

## What it leaves alone

The `text`, `textarea`, `number`, `currency`, `measurement`, `select`, `checkbox`, `radio`, and `date` rules, and every handling of decision 0350. The drafter of screens (which does not draft a custom control). A control inside a closed shadow root, which the base cannot enter.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A script of the engine's own, run in the page, to set each control's state | It puts engine code in the page's world, and each script depends on the page's framework |
| A file input given a path from the caller | The caller steers where the engine reads (spec 502); the staged name has no way out of its folder |
| The tree's path as a list | A list output is read from a column of a table; a text keeps one value one value, and the `/` limit is named |
| A grid read in an acting tab so the finder can be typed into | A read that may write by accident (decision 0195); the network is the other way to such a grid |
| One generic rule that clicks the label and reads a value | A page that keeps its value elsewhere than the element would seem to take it; each rule reads the page's own state and stops when it cannot |

## Proof

In `crates/a2m-pack-web-ui/tests/controls.rs`, through the driver and the screen `controls` of `tests/fixtures/cases`, against the page `/cases/controls` of the test application, each asking the application what its page was told: every control but the file and the grid set, saved, and read back, with the page's own state compared (`each_custom_control_is_set_saved_and_read_back`); a slider moved up and down and one that cannot take its value; a staged file attached, and a path, an absent name, and a folder refused with nothing sent; a grid cell edited and read by its row's key and a row that is not there; a virtualized grid read by key where the page holds the row and refused where it does not; a choice with no rule, a tree path that is not there, and a picker-only date, each a named stop that tells the page nothing; and the compiler's refusals of the new keys. In `crates/a2m-pack-web-ui/src/driver.rs`, a test that every registered control has a rule and that another name is a stop.

## What would reopen it

A page of the test application, or of a real application, whose control keeps its state somewhere that none of the attributes above says; or a file the driver must take from a place other than the staging folder.
