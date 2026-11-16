# 34. First target archetypes

Three kinds of first target prove the design in different situations. No real application is named here or anywhere in this repository; the files in the repository use an imaginary application called `example` and the built-in test application.

|  | A web app with no known API | An app with a published API | A desktop app |
| --- | --- | --- | --- |
| Starting mode | `ui`, with `page_http` and `http` bindings mined and promoted over time | `api`, or `mcp` pass-through if the app's own MCP server covers the job | `desktop` |
| Identity | The user's existing sign-in in Solo; service account or per-user modes on a server | API token or OAuth from the vault | The signed-in desktop user |
| First pipeline | Enter the rows of a spreadsheet as records | Create or update records from a source list | A read-only export |
| Record key | A code that is unique within its parent record | The record id, or a custom field holding the source id | App-specific |
| Hardest part | Units, fractions, and rounding in read-back; values recalculated after save | Rate limits; features the API lacks | Accessibility quality of the app |

The first thing to learn about a web app costs ten minutes: open it in a browser, open the developer tools' Network tab, save a record, and see whether the page sends JSON to an API, posts HTML forms, or neither. That answer decides how far the HTTP modes can go.

Wrapping any vendor's software should be cleared with the vendor's terms and with IT before it is used on real work.

Target files for real applications are kept outside this repository, in a private location the owner chooses.
