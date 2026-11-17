# The file stand

Files in a temporary folder, for the file pack of [spec section 308](../../docs/spec/3-packs/308-the-file-pack.md). The files belong to an imaginary order-entry system named `example`. The pack is proven against this stand and never against anyone's real files. The rules every stand follows are in [the architecture](../../docs/architecture/other-parts.md) and in the [test application plan](../../docs/plan/test-application.md).

The stand is a library with no server and no runtime. It depends on no product crate, and a product crate may use it in tests only.

## Using it

```rust
use example_files::faults::{Change, Fault};
use example_files::harness::FileStand;
use example_files::seed;

let stand = FileStand::start()?;              // a new folder, every seed file written
let root = stand.root();                      // the allowed root to give the pack
let file = stand.path(seed::STOCK_CSV)?;      // the real path of one seed file

// ... the pack reads; then another writer gets in first:
stand.set_fault(Fault::changed(seed::STOCK_CSV, Change::ThePlace))?;
// ... the pack saves, or stops as stale; then ask what is on disk:
let truth = stand.inspect(seed::STOCK_CSV)?;  // kind, size, digest, identity, links
let cells = stand.stored_cells(seed::STOCK_CSV, ',', "item_code", "EX-1002", "on_hand")?;
let count = stand.write_count(seed::STOCK_CSV)?;
assert!(stand.strays()?.is_empty());          // no temporary file was left behind
```

Each stand has its own folder under the system's temporary folder, and removes it when it is dropped. Two stands share nothing. A name is a path below the root with `/` between its parts on every operating system; `path` turns it into a real path and refuses a name that would leave the root.

`root()` has every link in its path resolved, so a check that a resolved path lies inside it compares like with like. On Windows that is the long form that begins `\\?\`.

## The seed files

Every file is written from code, byte by byte, at start and at reset. `seed::files()` gives the same bytes to a test. The constants are in `example_files::seed`.

| Constant | Name below the root | Format | What makes it hard |
| --- | --- | --- | --- |
| `STOCK_CSV` | `prices/stock.csv` | Delimited, comma | LF, the last line ended, no byte-order mark. A delimiter inside quotes, a doubled quote, a line break inside a field, an empty field in quotes, a number padded with zeros (`0040`) and one padded with spaces, a narrow no-break space, a field ending in a space, and a note aimed at an agent |
| `PRICE_LIST_CSV` | `prices/price-list.csv` | Delimited, comma | A byte-order mark, CRLF, the last line not ended, a quoted number holding a comma |
| `NON_ASCII_NAME_CSV` | `prices/价格表-ß.csv` | Delimited, semicolon | A name and a content that are not ASCII. The name has one normal form, so no file system rewrites it |
| `STOCK_DUPLICATE_KEY_CSV` | `prices/stock-duplicate-key.csv` | Delimited, comma | Two rows share the key `EX-1002` |
| `BROKEN_CSV` | `prices/broken.csv` | Not valid | A quote is opened and never closed |
| `NOT_UTF8_CSV` | `prices/not-utf8.csv` | Not valid | One byte of a single-byte encoding |
| `PRICE_LIST_WORKBOOK` | `prices/price-list.xlsx` | Workbook | Sheets `Prices` and `Notes`; the table `price_table` in `A1:E4`; the name `standard_discount` for `Notes!B1`; a column of formulas over the price and stock columns, each with its last result; a shared table of strings; a hidden column; a merged range; a date kept as the serial `46343`; a number spelled `0040` |
| `PRICE_LIST_MACRO_WORKBOOK` | `prices/price-list-macro.xlsm` | Workbook | The same, with a macro part of opaque bytes |
| `PRICE_LIST_SIGNED_WORKBOOK` | `prices/price-list-signed.xlsx` | Workbook | The same, with the parts that mark a signed package. The signature is not a real one |
| `PRICE_LIST_PROTECTED_WORKBOOK` | `prices/price-list-protected.xlsx` | Not a package | The first bytes of the container an encrypted workbook is kept in, then filler. It is not a real encrypted file |
| `SETTINGS_JSON` | `config/example-settings.json` | JSON | LF, two-space indent, three list elements each spaced differently, numbers spelled `2.50`, `1.9e1`, `-0`, `1000.0`, escapes that need not be escapes, and a note aimed at an agent |
| `UNITS_JSON` | `config/units.json` | JSON | One line, no spaces, no line ending |
| `DUPLICATE_KEY_JSON` | `config/duplicate-key.json` | JSON | One key twice in one object |
| `BROKEN_JSON` | `config/broken.json` | Not valid | The file stops before its brackets close |
| `SETTINGS_TOML` | `config/example.toml` | TOML | Comments, a table, a list of tables, two quoting styles |
| `SETTINGS_YAML` | `config/example.yaml` | YAML | Comments, an anchor and an alias, two documents |
| `SETTINGS_XML` | `config/example.xml` | XML | Two namespaces, a comment, an entity reference, a literal section, attributes in two orders |
| `ORDERS_TSV` | `exports/orders.tsv` | Delimited, tab | An empty field |
| `ORDERS_FIXED_WIDTH` | `exports/orders-fixed-width.txt` | Text document | Fixed-width columns, CRLF |
| `LARGE_CSV` | `exports/order-lines-large.csv` | Delimited, comma | 50,000 rows, more than a megabyte |
| `EMPTY_CSV` | `exports/empty.csv` | Delimited, comma | No bytes |
| `TERMS_TEXT` | `documents/terms.txt` | Text document | Trailing spaces, the last line not ended |
| `ARCHIVED_STOCK_CSV` | `archive/2026/11/stock-2026-11.csv` | Delimited, comma | Three folders down |

Beside the root, outside it, is a folder holding one file, `outside.csv`. Nothing under test may read or change it. `outside_folder()` gives its path and `outside_untouched()` says whether it is as it was.

`seed::workbook(&seed::Workbook { .. })` builds a workbook with other contents: another price, another layout, a formula at a declared value, a macro part, a signature. Every part is stored without compression, in a fixed order and with a fixed date, so the same options give the same bytes.

## The control interface

Every answer is read from disk at the moment it is asked for. A link is reported as a link and never followed. No fault ever stops the stand answering.

| Call | What |
| --- | --- |
| `reset()` | Restore the seed files; end every fault; start the ledger again |
| `inspect(name)` | A `FileTruth`: what is there (absent, file, folder, link, other), its size, its SHA-256 digest as 64 lowercase hexadecimal characters, whether it is read-only, and on Unix its identity, how many links it has, its owner and group, and its permission bits |
| `bytes(name)`, `sha256(name)`, `exists(name)` | The bytes of a regular file, their digest, and whether anything is there |
| `listing()` | Every file and link below the root, by name, in order |
| `strays()` | Files and links that are neither seed files nor made by a fault: what the thing under test left behind |
| `stored_rows(name, delimiter)` | The rows of a delimited file, each field as its text |
| `stored_cells(name, delimiter, key_header, key, header)` | The text under `header` in every row whose `key_header` holds `key`. Two rows with one key give two texts |
| `stored_json_values(name, &[JsonStep])` | Every value a path reaches. A string comes back as its content; a number, `true`, `false`, and `null` exactly as the file spells them. A key written twice gives two values |
| `package_parts(name)` | The parts of a workbook's package: name, how it is kept, checksum, sizes, and the digest of its bytes as they sit in the file. A part a save left untouched has the same digest before and after |
| `observe()` | Look at every file; return and count each difference from the last look |
| `write_count(name)`, `write_counts()` | Look, then give the counts since the last reset |
| `set_fault(fault)`, `faults()`, `clear_faults()` | Switch a fault on, list those set, end the standing ones |
| `escape_attempts()` | Path texts that try to reach the file outside the root |
| `outside_untouched()` | Whether the folder outside the root is exactly as it was |

**The stand's own readers.** `stored_rows`, `stored_cells`, and `stored_json_values` read through code in this crate (`example_files::reader`) that shares nothing with the product. It reads delimited text and JSON. It does not read a value out of a workbook, YAML, TOML, or XML: for those the stand reports bytes, digests, and, for a workbook, parts.

**Counting writes.** The stand has no watcher. It keeps a ledger of what it saw at its last look: each file's digest and, on Unix, its identity. A difference between two looks counts once, so two saves between two looks count as one, and a save of the same bytes is seen only on Unix. `WriteCount` holds `content_changes`, `appearances`, `disappearances`, and `replacements`. `replacements` is how many times the file was another file under the same name; it is `None` on Windows, where safe code cannot read a file's identity, so there the stand cannot tell a replace from an edit in place. What the stand's own faults do to a file is not counted.

## Faults

Every fault is a value of `example_files::faults::Fault`, given to `set_fault`. A **standing** fault holds until `clear_faults()` or `reset()`. An **act** happens once, when it is set, and what it left on disk stays until `reset()`. A fault that cannot be shown here makes `set_fault` return `StandError::Unsupported`, which `is_unsupported()` recognises, so a test ends early instead of failing.

| Fault | Kind | What the stand does | Where |
| --- | --- | --- | --- |
| `ChangedByAnother { file, change, how }` | Act | Another writer changes the file. `how` is `Replace`, a new file put in the old one's place, or `InPlace` | Everywhere |
| `FileRemoved { file }` | Act | The file is gone | Everywhere |
| `FolderRemoved { folder }` | Act | The folder disappears, with everything in it | Everywhere |
| `PartialWrite { beside, temp_name }` | Act | A file named `temp_name` appears beside the file, holding the first half of its bytes | Everywhere |
| `LockSignal { sibling }` | Standing | An empty file with that name exists. `Fault::lock_signal_for(file)` names it `<file>.lock` | Everywhere |
| `HeldWithoutSharing { file }` | Standing | The stand holds the file open and shares it with nobody | Windows only |
| `ReadOnlyFile { file }` | Standing | The file is marked read-only. On Windows the mark stops an edit in place by anyone. On Unix it stops an edit in place by an ordinary user, and never a replace, because the folder decides that | Everywhere |
| `ReadOnlyFolder { folder }` | Standing | Nothing can be made, replaced, or removed in the folder: a save fails for want of permission | Unix, and not when the tests run as the superuser |
| `LinkOut { at }` | Act | A link below the root points at the file outside it | Unix; Windows where the user may make links |
| `FolderLinkOut { at }` | Act | A link below the root points at the folder outside it | Unix; Windows where the user may make links |
| `SecondLink { file, at }` | Act | The file gets a second name, so it has two links | Everywhere |
| `LinkInside { at, to }` | Act | A link below the root points at another name below the root, where nothing need be | Unix; Windows where the user may make links |
| `MovedBehindLink { name, to }` | Act | A file or a folder is moved to another name below the root, and a link that points at it is left under its old name | Unix; Windows where the user may make links |
| `OwnedByAnother { file }` | Act | The file is given to another owner and another group | Unix, and only when the tests run as the superuser |
| `FileMode { file, mode, denies_this_user }` | Standing | The file has exactly these permission bits. With `denies_this_user`, the stand tries to write the file itself and calls the fault unsupported if it can | Unix; with `denies_this_user`, not when the tests run as the superuser |

`Change` is what another writer changes:

| Change | What |
| --- | --- |
| `ThePlace` | The value at the place the cases write to, below |
| `Elsewhere` | A value somewhere else, the place left as it was |
| `Layout` | A column, a key, or a sheet gets another name |
| `Bytes { find, replace }` | The first occurrence of some bytes becomes others |
| `Whole(bytes)` | The whole file becomes these bytes |

| File | The place | `ThePlace` | `Elsewhere` | `Layout` |
| --- | --- | --- | --- | --- |
| `prices/stock.csv` | `on_hand` of `EX-1002` | `40` becomes `38` | `on_hand` of `EX-1001`: `12` becomes `11` | `on_hand` becomes `quantity_on_hand` |
| `prices/price-list.csv` | `unit_price` of `EX-1002` | `10.00` becomes `11.00` | `unit_price` of `EX-1001`: `4.50` becomes `4.75` | `unit_price` becomes `price` |
| `config/example-settings.json` | `cutoff_time` of the warehouse `W2` | `14:00` becomes `13:00` | `schema_version`: `3` becomes `4` | Every `cutoff_time` becomes `cutoff` |
| `prices/price-list.xlsx` | The unit price of `EX-1002`, cell `C3` | `10` becomes `11` | The stock of `EX-1001`, cell `D2`: `12` becomes `11` | The sheet `Prices` becomes `Price list`; the header `Unit price` becomes `Price` |

`Fault::formula_at_declared_value()` saves the workbook with a formula in `C3`.

## The cases of section 308

| # | Case | In the stand |
| --- | --- | --- |
| 1 | A workbook, a JSON file, and a delimited file | The seed files |
| 2 | Open and save with no change | `sha256` before and after; `package_parts` part by part |
| 3 | The engine stops between the replace and its record | `inspect` shows the old digest or the new one. Stopping the engine is the test's part |
| 4 | Another writer changes the file, in the place and elsewhere | `Change::ThePlace`, `Change::Elsewhere` |
| 5 | Held open without sharing; the lock signal | `HeldWithoutSharing`, `LockSignal` |
| 6 | A formula at a declared value; a formula that depends on a written cell | `Fault::formula_at_declared_value()`; column E of the seed workbook |
| 7 | A macro part, a signature, an encrypted file | Three seed files |
| 8 | A link out of the root; a file with two links; a link that stays inside the root | `LinkOut`, `FolderLinkOut`, `SecondLink`, `LinkInside`, `MovedBehindLink` |
| 9 | Another layout | `Change::Layout` |
| 10 | A value the application would store differently | The date serial, the padded numbers, and the narrow no-break space in the seed files |
| 11 | Text aimed at an agent | The notes in `stock.csv` and `example-settings.json` |
| 12 | Two rows with one key | `stock-duplicate-key.csv`, `duplicate-key.json` |

Beyond the section: a partial write left behind, a read-only file and folder, a file of another owner or with other permission bits, a file and a folder that disappear, texts that try to leave the root, files that are not valid in their format, a name that is not ASCII, a large file, and an empty one.

## What the stand does not do

- **A full disk.** Nothing portable fills a disk on demand. `ReadOnlyFolder` shows a save that fails for want of permission, on Unix only; `storage_full` has no fault.
- **A change in the window between the pack's last digest check and its replace.** The stand acts when it is told to, not at a moment inside the pack.
- **Change notifications.** Counts come from looks, as above.
- **A value at a place in a workbook, YAML, TOML, or XML,** as above.
- **A real signature, a real macro, a real encrypted file.** Each is the least that lets a reader see what kind of file it is.
- The workbook was opened once, by hand, with an independent reader, which found both sheets, the table, the name, the merged range, the formulas, and their cached results. No test in this crate does that.
