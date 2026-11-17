name: pack-web

# The web pack, mode `ui`

**Latches onto.** A web application that a person uses in a browser. The engine starts Chrome, Edge, or Chromium with a data directory it owns. Surface kind: `web`, with `origin`.

**File kind.** `screens/`: a screen model. It holds every locator for the application, and nothing else does.

**Verbs.** `read`, `list`, `create`, `update`, `upsert`, `submit`.

**Steps.** `goto`, `wait`, `read`, `set`, `invoke`. Controls built so far: `text`, `textarea`, `number`, `currency`, `measurement`, `select`, `checkbox`. Any other control is refused.

Not built: the modes `page_http` and `site_tools`, platform profiles, and the screen matcher.

## A screen, from the `example` target

```yaml
screen: line_item
surface: app
reach: {url: "/orders/{order_id}/items/{item_code}"}
ready: {visible: {role: heading, name: "Line Item Details"}}
fields:
  quantity:
    type: integer
    control: number
    locate:
      - {role: textbox, name: Quantity}
    fingerprint: {section: {role: form, name: ""}, near_text: "Line Item Details"}
actions:
  save:
    locate:
      - {role: button, name: Save}
    fingerprint: {section: {role: form, name: ""}}
    confirm: {text: {locate: {role: status, name: ""}, equals: "Saved."}}
    commits: true
variants:
  not_found:
    when: {visible: {role: heading, name: "Not found"}}
    handling: absent
```

A binding of mode `ui` names a `screen`, and for a write a `verb`, `fields` (screen field to expression), an `action`, and a `read_back` such as `{screen: line_item}`.

## Rules

- `locate` is a list of locators, such as `{role: textbox, name: Quantity}` or `{label: Description}`. A `css` locator comes last in the list, and draws a warning.
- Every field, action, and table states a `fingerprint`. The lint `web.no_fingerprint` is an error.
- A variant says what an unexpected state means: `absent` for a missing record, `stop` otherwise. A state with no variant is `unknown_variant`.
- Wait on a named condition. There is no fixed sleep.

## Refuses

- A browser the engine did not start, and anyone's everyday profile.
- Hiding that it is automation, changing fingerprints, answering a challenge. A block is `edge_blocked`.
- Clicking by position or picture, and XPath.
- An origin the target did not declare.
- An unrecognized page, dialog, control, or platform.
- Reading a password field (`password_read_refused`).

## Codes

`drift`: `locator_not_found`, `locator_ambiguous`, `fingerprint_mismatch`. `unknown_variant`: `unexpected_dialog`, `unexpected_page`, `unexpected_control`, `unknown_control`, `browser_not_owned`, `read_too_large`. `rejected`: `app_validation_error`. `policy`: `surface_not_declared`, `password_read_refused`. `busy`: `browser_unavailable`. `input`: `step_not_usable`, `address_invalid`, `address_value_refused`.

A crash or a lost connection after a commit was sent is `possible_commit`, never `transient`.
