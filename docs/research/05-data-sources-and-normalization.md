# Research: reading input data and comparing values exactly

**Status:** completed by a research agent on 2026-11-15. Feeds [spec section 210](../spec/2-kernel/210-sources-and-value-normalization.md).

## Question

What are the pitfalls in reading spreadsheet inputs deterministically, mapping values declaratively, and normalizing values so a read-back comparison is exact?

## Findings

1. **Formulas are not computed.** openpyxl never evaluates formulas. With `data_only` it returns the value Excel cached when it last saved. A file written by a tool that does no calculation can have no cached value at all. [openpyxl formulae](https://openpyxl.readthedocs.io/en/3.1/simple_formulae.html)
2. **Dates are just numbers.** A date is a serial number; only the cell's number format makes it a date. The 1900 and 1904 date systems are 1,462 days apart. Excel's 1900 system wrongly treats 1900 as a leap year. [openpyxl dates](https://openpyxl.readthedocs.io/en/3.1/datetime.html), [Excel date systems](https://support.microsoft.com/en-us/office/date-systems-in-excel-e7fe7167-48a9-4b96-bb53-5612a800b487)
3. **Value, text, format, and formula are separate.** SheetJS keeps them apart (`v`, `w`, `z`, `f`). Its formatted text follows slightly different rules from Excel's display, and Excel's built-in short date format changes with the reader's system settings. [SheetJS cell](https://docs.sheetjs.com/docs/csf/cell), [SheetJS dates](https://docs.sheetjs.com/docs/csf/features/dates)
4. **Merged cells, hidden rows, named ranges.** In a merged range only the top-left cell holds data. Hidden rows and columns are flags to read explicitly. Named ranges can belong to the workbook or one sheet. [openpyxl merge](https://openpyxl.readthedocs.io/en/3.1/api/openpyxl.worksheet.merge.html), [defined names](https://openpyxl.readthedocs.io/en/stable/defined_names.html)
5. **Readers can change values silently.** By default pandas `read_excel` turns strings such as "NA", "None", "null", and "n/a" into missing values. [pandas](https://pandas.pydata.org/docs/reference/api/pandas.read_excel.html)
6. **Displayed is not stored.** Excel keeps 15 significant digits, and what a cell shows can differ from what it stores. [Microsoft](https://learn.microsoft.com/en-us/troubleshoot/microsoft-365-apps/excel/floating-point-arithmetic-inaccurate-result)
7. **Expression languages differ in safety.**
   - CEL always finishes, runs in linear time, never converts number types implicitly, and has no decimal type. [CEL](https://github.com/cel-expr/cel-spec/blob/master/doc/langdef.md)
   - JSONata can loop forever, so it needs a stack limit and a timeout; its `$round` rounds halves to even. [JSONata](https://docs.jsonata.org/guardrails)
   - JMESPath has only JSON types and no arithmetic. [JMESPath](https://jmespath.org/specification.html)
8. **Rounding.** Binary floats cannot hold most decimals exactly. Use decimal arithmetic built from strings. [Python docs](https://docs.python.org/3/tutorial/floatingpoint.html)
9. **Locale and Unicode.** A Unicode locale-data update changed the French thousands separator from U+00A0 to U+202F, which broke string comparisons between machines. NFKC normalization rewrites compatibility characters; NFC does not. [Haacked](https://haacked.com/archive/2020/05/17/french-thousand-separator-mystery/), [Babel](https://babel.pocoo.org/en/latest/api/numbers.html)
10. **Reading values back from the browser.** A checkbox's `checked` attribute is its starting state, not its current state. A select's value is its stored value, not its label. [MDN](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/input/checkbox), [Playwright Locator](https://playwright.dev/docs/api/class-locator)
11. **Units.** Pint's built-in unit file has no prime or double-prime names for feet and inches. [Pint](https://github.com/hgrecco/pint/blob/master/pint/default_en.txt)
12. **Validation tools.** Pydantic's strict mode refuses to turn "123" into an integer. Frictionless Table Schema already defines field properties for decimal and grouping characters, true and false values, and missing values. [Pydantic](https://pydantic.dev/docs/validation/latest/concepts/strict_mode/), [Table Schema](https://specs.frictionlessdata.io/table-schema/)

## Recommendations adopted in the spec

1. **Source readers.** Read both formulas and cached values; fail when a formula cell has no cached value. Record the workbook's date system, each cell's number format, merged ranges, and hidden flags. Find inputs by named range or header text, never by fixed coordinates. Pin reader versions.
2. **Template fingerprint.** Hash sheet names, named ranges and their targets, header text, merged ranges, and dropdown lists. Reject a workbook whose fingerprint has no known template version.
3. **Mapping layer.** Mappings in schema-validated files. CEL for conditions and simple transforms. Number and unit conversions in a small, tested set of host functions. Decimals passed as strings or whole numbers of the smallest unit.
4. **Field type carries:** kind; normal form; decimal places and rounding mode; unit, display unit, and fraction step for measurements; currency and decimals for money; a time-zone rule for dates; parse and display locale; string rules (NFC, whitespace, non-breaking spaces, case); a code-to-label map for choices; and how to read the value back.
5. **Comparison.** Convert both sides into strict typed values and compare those, never display strings. Keep raw and normalized values as evidence.

## Still uncertain

- Whether each target app rounds or reformats a value when it is saved; this must be checked field by field.
- How to tell that a cached formula value is stale without recalculating the workbook.
- How a units library handles fractions such as 6 1/2 inches.

## Note on Rust

The research examined Python and JavaScript libraries. The pitfalls are properties of the file format and apply equally to a Rust reader. The Rust xlsx reader, decimal, and CEL crates must be chosen and verified in Phase 0 and Phase 3.
