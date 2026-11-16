# 13. Interface flavors

Web apps are built in very different ways. Three engine mechanisms absorb the differences, so operation and pipeline files look the same whatever the app is built with.

- **Capability probe.** At setup, the probe loads the app and matches Wappalyzer-style rules (globals, DOM markers, script names, headers, cookies) to name the platform, with a confidence level. It assigns a support tier: full, read-only, API-preferred, or unsupported.
- **Platform profile.** One preset per platform: a readiness check, locator priority, frame and shadow scopes, regexes for generated ids to reject, the input commit strategy, and how often the vendor ships UI changes.
- **Widget drivers.** One tested driver per control type, each with its own set, read-back, and failure rules.

| Flavor | What makes it hard | Profile and driver behavior |
| --- | --- | --- |
| Server-rendered pages | Each action reloads the page | Wait on load; forms carry their own hidden tokens |
| ASP.NET WebForms, JSF | Generated ids; partial postbacks re-render the form | Locate by label; wait until the async postback reports finished ([Microsoft](<https://learn.microsoft.com/en-us/previous-versions/bb383989(v=vs.100)>)) |
| React | Setting `value` directly never reaches app state | Escalate to the native setter plus events ([reference](https://coryrylan.com/blog/trigger-input-updates-with-react-controlled-inputs)) |
| Angular | Asynchronous change detection | Wait on the Testability stable signal when the app exposes it ([Angular](https://angular.dev/api/core/Testability)) |
| SAP Fiori / UI5 | Ids like `__button2` shift with load order | Control-level locators by view and control type; never generated ids ([SAP](https://ui5.github.io/docs/05_Developing_Apps/id-handling-in-openui5-the-complete-guide-f51dbb7.html)) |
| Salesforce Lightning | Shadow DOM, iframes for Visualforce, three releases a year | Role and label locators; frame paths; higher drift-check frequency ([Salesforce](https://developer.salesforce.com/docs/platform/lwc/guide/create-dom.html)) |
| Power Apps, Dynamics 365 | Canvas apps inside a host iframe; grids by row index | Microsoft's locator order: role, label, test id, control name ([Microsoft](https://learn.microsoft.com/en-us/power-platform/developer/playwright-samples/locators)) |
| Virtualized grids (AG Grid, Kendo) | Only visible rows exist in the page | Grid driver addresses cells by row key and column id, scrolling or filtering to find them; reads prefer the network ([AG Grid](https://www.ag-grid.com/javascript-data-grid/testing.md)) |
| Shadow DOM | Hidden from ordinary selectors | Open roots are pierced automatically; closed roots fall back to keyboard or network ([Playwright](https://playwright.dev/docs/locators)) |
| Flutter web | Canvas drawing; no elements by default | Turn on the semantics tree, then use it; otherwise read-only through the network ([Flutter](https://docs.flutter.dev/ui/accessibility/web-accessibility.md)) |
| Canvas editors (Google Docs style) | Nothing to locate | API-preferred tier ([Google](https://workspaceupdates.googleblog.com/2021/05/Google-Docs-Canvas-Based-Rendering-Update.html)) |
| Electron desktop apps | Not in a browser tab | Attach over the app's DevTools port; fails if the app disables the inspector ([Playwright](https://playwright.dev/docs/api/class-electron)) |
| Internet Explorer mode apps | Not drivable over the DevTools protocol | Unsupported tier; would need a separate Selenium-based back end ([Microsoft](https://learn.microsoft.com/en-us/microsoft-edge/webdriver/ie-mode)) |
| Pop-ups, new tabs, downloads | The action leaves the page | Declared as an expected event on the step; undeclared ones stop the run |
| A/B tests, flags, languages | Same page differs between sessions | Fixed locale; ordered locators; an unknown variant stops the run |

Not yet verified against vendor documentation: ServiceNow, Workday, Oracle APEX and ADF, DevExpress grids, and legacy framesets. Their profiles start in the read-only tier until tested.
