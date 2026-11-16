# Research: interface flavors and their automation pitfalls

**Status:** completed by a research agent on 2026-11-15. Findings come from pages the agent opened unless marked otherwise. Feeds [spec section 13](../spec/13-interface-flavors.md).

## Question

What are the documented automation pitfalls and proven techniques for each kind of web interface, and what should a platform profile contain for each?

## Findings

- **ASP.NET WebForms.** An UpdatePanel partial postback is observable through `Sys.WebForms.PageRequestManager.getInstance().get_isInAsyncPostBack()`, which gives a deterministic wait. [Microsoft](https://learn.microsoft.com/en-us/previous-versions/bb383989(v=vs.100))
- **React.** Assigning `input.value` goes through React's overridden setter, so state never updates. The fix is the native `HTMLInputElement.prototype` value setter plus a bubbling event; checkboxes and radios need `click`. [Cory Rylan](https://coryrylan.com/blog/trigger-input-updates-with-react-controlled-inputs)
- **Angular.** `Testability.whenStable` and `isStable` are the stability hooks, but apps started with `bootstrapApplication` lack them unless `provideProtractorTestingSupport()` is added. [Angular](https://angular.dev/api/core/Testability)
- **Shadow DOM.** Playwright locators pierce open shadow roots; closed roots are unsupported and XPath does not pierce. Locators are strict. [Playwright](https://playwright.dev/docs/locators)
- **SAP UI5 / Fiori.** Auto-generated ids such as `__view0` and `__button2` change with structure and load order. SAP says the DOM is not an API and to prefer control locators (id, viewName, controlType). [UI5 id handling](https://ui5.github.io/docs/05_Developing_Apps/id-handling-in-openui5-the-complete-guide-f51dbb7.html), [UIVeri5 locators](https://cdn.jsdelivr.net/npm/@ui5/uiveri5@1.49.12/docs/usage/locators.md)
- **Salesforce.** Lightning Experience uses synthetic shadow; Lightning Web Security enforces closed shadow roots. Ids are unstable, Visualforce sits in iframes, and three releases a year shift the DOM. [Salesforce](https://developer.salesforce.com/docs/platform/lwc/guide/create-dom.html), [Gearset](https://gearset.com/blog/salesforce-ui-testing-challenges/)
- **Power Apps and Dynamics 365.** Microsoft ranks role, then label, then test id, then `data-control-name`. Canvas apps live in `iframe[name="fullscreen-app-host"]`; model-driven grids use `row-index`, not `nth-child`. [Microsoft](https://learn.microsoft.com/en-us/power-platform/developer/playwright-samples/locators)
- **Grids.** AG Grid ships test ids (`setupAgTestIds`, `agTestIdFor.cell(rowId, colId)`). Kendo virtual scroll renders only visible rows plus a buffer. [AG Grid](https://www.ag-grid.com/javascript-data-grid/testing.md), [Kendo](https://www.telerik.com/kendo-angular-ui/components/grid/scroll-modes/virtual)
- **Flutter web.** The semantics DOM is off until the hidden `aria-label="Enable accessibility"` button is pressed or the app calls `ensureSemantics()`. [Flutter](https://docs.flutter.dev/ui/accessibility/web-accessibility.md)
- **Google Docs.** Canvas rendering broke DOM-dependent extensions; Google recommends Workspace APIs. [Google](https://workspaceupdates.googleblog.com/2021/05/Google-Docs-Canvas-Based-Rendering-Update.html)
- **Electron.** Playwright support is experimental, needs Electron v21 or later, and fails if the `nodeCliInspect` fuse is disabled. [Playwright](https://playwright.dev/docs/api/class-electron)
- **IE mode.** Automated only through Selenium IEDriver 4+ attaching to Edge, not Playwright; window handles are unreliable. [Microsoft](https://learn.microsoft.com/en-us/microsoft-edge/webdriver/ie-mode)
- **Detection.** Wappalyzer-style rule sets match on `js`, `dom`, `scriptSrc`, `meta`, `headers`, and `cookies`, plus `implies`, `requires`, and `excludes`. [webappanalyzer](https://github.com/enthec/webappanalyzer)

## Recommendations adopted in the spec

1. **Profile schema:** a readiness predicate (JS expression), locator priority, frame and shadow scopes, an id-stability rule (regexes for generated ids to reject), an input commit strategy, and a release-drift risk rating.
2. **Probe:** reuse the Wappalyzer rule shape. Output platform, confidence, and a support tier: DOM-native, semantics-only (Flutter), API-preferred (Docs), unsupported (IE mode, applets, closed shadow).
3. **Text driver:** fill, read back the value, then escalate to key-by-key typing, then the native-setter fallback.
4. **Grid driver:** address cells by (row key, column id); scroll until found; never use DOM position.
5. **Routing:** send IE-mode apps to a separate Selenium/IEDriver back end, and prefer vendor APIs for canvas editors.

## Not verified

- JSF, ServiceNow (its docs page failed to load), Workday, Oracle APEX and ADF, DevExpress, framesets, applets and ActiveX, and hydration timing.
- Concrete detection markers (`ng-version`, `__VIEWSTATE`, `window.sap.ui`, `$A`, `Xrm`, `apex`, `flutter-view`) are from general knowledge, not checked against fingerprint files.
- AG Grid's page did not cover virtualization waits.
- Which events a programmatic fill dispatches was not stated in the Playwright docs read.
