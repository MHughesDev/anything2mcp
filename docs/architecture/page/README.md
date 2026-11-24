# The architecture page

The source of A2MArchitecture, the interactive page that the owner reads the design from. It is published as an artifact outside this repository; these files are what it is built from.

| File | What it is |
| --- | --- |
| `arch.json` | The data: every part, its place in a view, its text, its state, its arrows and what each carries, and the walks through each view |
| `outline.json` | The first screen: five stations on one line, the route a call takes, and three shelves beneath it. Each names the parts of the root view it holds, with one sentence of its own |
| `template.html` | The page. It shows the line and nothing else at first; a station opens a sheet, and every part on a sheet opens its own, as deep as the data goes. A switch beside the line (Line, Graph) shows the same parts and arrows as a graph instead: the stations hang from the line, a part unfolds into its inner parts in place, and a click opens the same sheet. The graph loads one script, d3 7.9.0 from cdnjs.cloudflare.com, only when a reader opens it; the line works without it |
| `build.py` | Checks the data and the outline and sets them into the template: `python3 build.py <output file>` from this folder. It fails when a view names a part that does not exist, when a part of the root view is in no station or in two, when a part cannot be reached from the first screen, when there are more than six stations, when an arrow does not join two different parts, or when a step of a walk lights an arrow its view does not have |
| `proposed-names.md` | Names coined for the page that no other document uses yet ([decision 0301](../../decisions/0301-every-part-has-a-proper-name.md)) |

Rules for keeping it true:

- A decision that changes the design changes this data in the same step, under the part it changes, with the alternative that lost.
- The title bar shows the whole build's percentage complete, counted by `tools/progress.py` from the roadmap when the page is built. The page is rebuilt and republished whenever a package is marked done.
- Every part has a proper name (decision 0301), and every arrow says what passes along it. The page shows a part's arrows as a list on its sheet, under "What it receives and hands over", and no longer draws them.
- The first screen stays short. A reader sees the line, the shelves, and the calls they can follow; text appears when they open something. Add detail to a part's sheet, never to the first screen.
- The state of a part (built, partly built, not built) follows the crate design and the roadmap. Many inner parts were given their state from the documents and not from the code; where the two differ, the code is right and the data is corrected.
- The text was drafted from the design documents by a worker and is not yet checked line by line. It describes; it does not rule. Where it disagrees with a specification section or a crate design, that document is right.
