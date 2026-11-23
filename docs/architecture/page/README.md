# The architecture page

The source of A2MArchitecture, the interactive drawing of the system that the owner reads the design from. It is published as an artifact outside this repository; these files are what it is built from.

| File | What it is |
| --- | --- |
| `arch.json` | The data: every part, its place in a view, its text, its state, its arrows and what each carries, and the walks through each view |
| `template.html` | The page, which draws any view from the data |
| `build.py` | Checks the data and sets it into the template: `python3 build.py <output file>` from this folder |
| `proposed-names.md` | Names coined for the page that no other document uses yet ([decision 0301](../../decisions/0301-every-part-has-a-proper-name.md)) |

Rules for keeping it true:

- A decision that changes the design changes this data in the same step, under the part it changes, with the alternative that lost.
- Every part has a proper name (decision 0301), and every arrow says what passes along it.
- The state of a part (built, partly built, not built) follows the crate design and the roadmap. Many inner parts were given their state from the documents and not from the code; where the two differ, the code is right and the data is corrected.
- The text was drafted from the design documents by a worker and is not yet checked line by line. It describes; it does not rule. Where it disagrees with a specification section or a crate design, that document is right.
