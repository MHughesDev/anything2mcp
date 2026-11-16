# 5. Core concepts

These are the nouns of the system. A coding agent writes the first eight; the system supplies the rest.

| Concept | What it is | Example |
| --- | --- | --- |
| Target | One application being wrapped | `example` |
| Surface | One way into a target: a web origin, an API base URL, an MCP server, a desktop window | `app.example.com`, `api.example.com`, the application's desktop client |
| Identity | Who the target sees: the user, or a dedicated account | A named user, signed in through Entra ID |
| Screen model | A named page, panel, or window: how to reach it, its fields and actions, how to know it is ready | `order.line_item_form` |
| Endpoint | A named backend call: method, path template, request and response schemas | `PUT /api/orders/{id}/items/{code}` |
| Field type | How a value is entered, read back, and compared: kind, unit, rounding, locale, control type | `length_ft_in` |
| Operation | One business action with typed inputs, outputs, and an effect class, defined once | `example_line_item_upsert` |
| Binding | One way to carry out an operation in one mode, with its own steps and verification | UI binding, HTTP binding |
| Pipeline | Operations in order over a data source, with checkpoints and success checks | `example_order_sheet_enter` |
| Source | A reader that turns an input file or another system's output into validated records | Spreadsheet reader |
| Driver | Engine code for one mode | Browser UI driver, HTTP driver |
| Auth provider | Engine code for one sign-in method | Entra broker provider |
| Session | A live, signed-in state for one identity on one target, shared by many runs | One user's session on `example` |
| Run | One execution of an operation or pipeline, with its ledger, trace, and result | `r_8f21` |
| Version | An immutable, tested, published snapshot of a target's files | `example@12` |

Built-in verbs: `open`, `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `upload`, `download`, `submit`, `call`. Each verb carries its own wait, commit, and verification rules in every driver, so an operation never spells them out.
