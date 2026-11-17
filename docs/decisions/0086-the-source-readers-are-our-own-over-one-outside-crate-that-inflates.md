# 0086. The source readers are our own, over one outside crate that inflates

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of the `a2m-sources` design on which outside xlsx reader keeps a number's text, and the three outside readers its facts table left "to be verified".

## Context

`a2m-sources` needs to read delimited text, JSON, and workbooks. Three things are asked of every reader, and they decide the choice:

1. A number is never read through a float ([spec section 210](../spec/2-kernel/210-sources-and-value-normalization.md)).
2. Every value comes with the place of its literal in the bytes, because the file pack patches a file by replacing exactly those bytes ([spec section 308](../spec/3-packs/308-the-file-pack.md)).
3. What a format does not allow stops the read with a named failure. Nothing is repaired.

## Decision

The csv reader, the JSON reader, the reader of a workbook's package, and the reader of the XML in its parts are this crate's own code. One outside crate is added.

| Crate | Version | Licence | Features | Why |
| --- | --- | --- | --- | --- |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 | `with-alloc` only | Inflates a deflated part of a workbook's package, with a limit on the output. Pure Rust, no C, so it builds on all three operating systems. Its tree is itself and `adler2` (0BSD OR MIT OR Apache-2.0). The whole tree passes the licence check |

What was weighed for each format, checked on crates.io on the date above:

| Format | Outside crate looked at | Why it is not used |
| --- | --- | --- |
| Workbook | `calamine` 0.38.0, MIT | It gives a number cell as a 64-bit float, so requirement 1 fails before our code sees the value. It gives no position of a value in a part |
| Workbook's package | `zip` 9.6.1, MIT | It would work. Reading a package is one directory of fixed-size records and two packing methods; our reader is about 200 lines, checks every checksum and declared size, and refuses an encrypted part by name. The crate brings a larger tree for the same result |
| Workbook's XML | `quick-xml` 0.42.0, MIT | It would work. A workbook's parts use elements, attributes, text, and five named characters. Our reader is about 250 lines, gives the span of every text, and by construction reads no document type and expands no declared entity, so two classes of attack on an XML reader cannot occur |
| Delimited text | `csv` 1.4.0, Unlicense OR MIT | It is lenient by design: a quote never closed or text after a closing quote is read as data, where requirement 3 needs a stop. It reports the place of a record, not of a field with its quotes |
| JSON | `serde_json` 1.0.154 with `arbitrary_precision` | The feature keeps a number's text, but a feature is switched on for the whole workspace, and it changes how every crate's numbers decode. It keeps the last of two equal keys silently, and gives no spans |

## Consequences

- The dependency tree of the kernel grows by two small crates.
- The four readers are ours to keep correct. Each has tests for what it refuses as well as what it reads, and the file stand's own reader, which shares no code with them, says what the seed files hold.
- Inflating is the one piece not written here. It is the piece where a mistake is a memory or a time problem rather than a wrong value, and the crate is small and widely used.
- If a format is added that is not simple, such as the older binary workbook, this decision does not cover it: it gets its own record.

## Alternatives not taken

- **An outside workbook reader, with the numbers re-read from the part.** Two parsers for one file, which is what "one parser, one meaning" in the crate design forbids.
- **Our own inflate.** About as much code as the package reader, with a much larger risk for no gain.
