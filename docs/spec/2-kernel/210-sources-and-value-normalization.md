# 210. Sources and value normalization

Input is read and validated in full before any surface of the target is touched, such as a browser opened in the web pack, and every comparison is between typed values, never display strings.

**Readers.** Built-in readers for xlsx, csv, json, and another operation's output. The xlsx reader:

- Reads each cell's formula and its cached value, which a workbook keeps side by side, and fails when a formula cell has no cached value. It never calculates a formula.
- Records the workbook's date system, each cell's number format, merged ranges, and hidden rows and columns.
- Finds inputs by named range or header text, never by fixed cell coordinates.
- Treats text such as "NA" or "null" as text, never as missing, unless the mapping says otherwise.

**Template fingerprint.** A hash of sheet names, named ranges and their targets, header text, merged ranges, and dropdown lists. A workbook whose fingerprint has no known template version is rejected with a list of differences.

**Mapping.** The reviewed source file maps source columns to named row fields, with CEL for conditions and tested host functions for conversions such as feet-and-inches parsing. A pipeline step passes row fields to an operation's inputs with `with`, so a source never names an operation. Decimals travel as strings or whole numbers of the smallest unit, never as floats, and an expression has no float type.

**Normalization rules** applied to both sides before comparison:

| Value kind | Rule |
| --- | --- |
| Numbers | Decimal arithmetic; places and rounding mode from the field type; display grouping stripped |
| Measurements | Converted to the field's base unit; fractions snapped to the declared step, such as 1/16 inch |
| Money | Currency and its decimal places fixed per field. A displayed amount may carry the field's currency code or a symbol written for that currency; another currency's code or symbol stops |
| Dates and times | Time-zone rule per field; Excel serial dates converted with the workbook's date system. A format in which digits follow a part of one or two digits, such as `YYYYMD`, does not compile: reading it would be a guess |
| Text | Unicode NFC; whitespace collapsed; non-breaking and narrow no-break spaces treated as spaces; case rule per field. Text compared without case is compared by its capital form, so text a system shows in capitals equals the text it was made from |
| Choices | Compared by stored code through a code-to-label map, never by display text. No text may stand for two choices: a code list in which a code is another choice's label, or two codes or two labels are alike, does not compile, because a value read back could then be the intended choice or the one that was there before |
| Checkboxes | In the web pack, read from the live checked state, not the HTML attribute |

The result of each comparison keeps both the raw and normalized values, so a mismatch report shows exactly what the system did to the value, for example rounding 32.5625 to 32.56.
