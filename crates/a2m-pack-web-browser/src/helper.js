// The in-page helper of the browser engine. It runs in an isolated world,
// which the page's own scripts cannot read or change. It locates elements
// by meaning, reads them, checks whether they can take input, and waits on
// named conditions with observers. It never polls, and it decides nothing:
// every answer goes back to the engine as plain data.
//
// Not part of a run's path in Rust terms, but on it all the same: nothing
// here may guess. An unknown query kind or control is an error, never a
// default.
(() => {
  if (globalThis.__a2m) return;

  const norm = (text) => (text || "").replace(/\s+/g, " ").trim();

  // Elements a person can perceive: laid out, and not hidden.
  const rendered = (el) =>
    el.isConnected &&
    el.getClientRects().length > 0 &&
    getComputedStyle(el).visibility !== "hidden";

  const hiddenFromTree = (el) => {
    for (let at = el; at; at = at.parentElement || (at.getRootNode() && at.getRootNode().host)) {
      if (at.getAttribute && at.getAttribute("aria-hidden") === "true") return true;
    }
    return false;
  };

  // Every element under a root, in document order, through open shadow roots.
  function* walk(root) {
    const start = root.nodeType === Node.ELEMENT_NODE ? root : null;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
    if (start && start.shadowRoot) yield* walk(start.shadowRoot);
    for (let el = walker.nextNode(); el; el = walker.nextNode()) {
      yield el;
      if (el.shadowRoot) yield* walk(el.shadowRoot);
    }
  }

  const INPUT_ROLES = {
    button: "button", submit: "button", reset: "button", image: "button",
    checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton",
    search: "searchbox", email: "textbox", tel: "textbox", text: "textbox",
    url: "textbox", password: "textbox",
  };
  const TAG_ROLES = {
    button: "button", textarea: "textbox", nav: "navigation", main: "main",
    h1: "heading", h2: "heading", h3: "heading", h4: "heading", h5: "heading", h6: "heading",
    table: "table", tr: "row", td: "cell", th: "columnheader", ul: "list", ol: "list",
    li: "listitem", form: "form", fieldset: "group", dialog: "dialog", option: "option",
    progress: "progressbar", output: "status", img: "img", article: "article",
    summary: "button", hr: "separator", p: "paragraph",
  };

  function roleOf(el) {
    const explicit = norm(el.getAttribute("role")).split(" ")[0];
    if (explicit) return explicit;
    const tag = el.localName;
    if (tag === "a" || tag === "area") return el.hasAttribute("href") ? "link" : "generic";
    if (tag === "input") {
      const type = (el.getAttribute("type") || "text").toLowerCase();
      if (el.hasAttribute("list") && (INPUT_ROLES[type] === "textbox" || type === "search")) return "combobox";
      return INPUT_ROLES[type] || "textbox";
    }
    if (tag === "select") return el.multiple || el.size > 1 ? "listbox" : "combobox";
    return TAG_ROLES[tag] || "generic";
  }

  const NAME_FROM_CONTENT = new Set([
    "button", "link", "heading", "cell", "columnheader", "rowheader", "option", "tab",
    "menuitem", "menuitemcheckbox", "menuitemradio", "checkbox", "radio", "switch",
    "treeitem", "tooltip", "row", "gridcell",
  ]);

  const byIds = (el, attribute) =>
    norm(
      (el.getAttribute(attribute) || "")
        .split(/\s+/)
        .filter(Boolean)
        .map((id) => {
          const root = el.getRootNode();
          const found = root.getElementById ? root.getElementById(id) : document.getElementById(id);
          return found ? found.textContent : "";
        })
        .join(" ")
    );

  const labelText = (el) =>
    el.labels && el.labels.length ? norm([...el.labels].map((label) => label.textContent).join(" ")) : "";

  // The accessible name, by the steps of the naming rules that business
  // forms use: labelled-by, label, native label, content, title, placeholder.
  function nameOf(el) {
    const labelledBy = byIds(el, "aria-labelledby");
    if (labelledBy) return labelledBy;
    const ariaLabel = norm(el.getAttribute("aria-label"));
    if (ariaLabel) return ariaLabel;
    const tag = el.localName;
    if (tag === "input") {
      const type = (el.getAttribute("type") || "text").toLowerCase();
      if (type === "button" || type === "submit" || type === "reset") {
        const value = norm(el.getAttribute("value"));
        if (value) return value;
        if (type === "submit") return "Submit";
        if (type === "reset") return "Reset";
      }
      if (type === "image") {
        const alt = norm(el.getAttribute("alt"));
        if (alt) return alt;
      }
    }
    const label = labelText(el);
    if (label) return label;
    if (tag === "img" || tag === "area") {
      const alt = norm(el.getAttribute("alt"));
      if (alt) return alt;
    }
    if (tag === "fieldset") {
      const legend = [...el.children].find((child) => child.localName === "legend");
      if (legend) return norm(legend.textContent);
    }
    if (tag === "table") {
      const caption = [...el.children].find((child) => child.localName === "caption");
      if (caption) return norm(caption.textContent);
    }
    if (NAME_FROM_CONTENT.has(roleOf(el))) {
      const content = norm(el.textContent);
      if (content) return content;
    }
    const title = norm(el.getAttribute("title"));
    if (title) return title;
    if (tag === "input" || tag === "textarea") return norm(el.getAttribute("placeholder"));
    return "";
  }

  function matcher(by) {
    switch (by.kind) {
      case "role_name":
        return (el) => !hiddenFromTree(el) && roleOf(el) === by.role && nameOf(el) === by.name;
      case "label":
        return (el) => (labelText(el) || byIds(el, "aria-labelledby")) === by.text && "value" in el;
      case "placeholder":
        return (el) => norm(el.getAttribute("placeholder")) === by.text;
      case "test_id":
        return (el) => el.getAttribute("data-testid") === by.id;
      case "title":
        return (el) => norm(el.getAttribute("title")) === by.text;
      case "css":
        return (el) => el.matches(by.selector);
      case "text": {
        const hit = by.contains
          ? (el) => norm(el.textContent).includes(by.text)
          : (el) => norm(el.textContent) === by.text;
        // The deepest element that carries the text, not each of its ancestors.
        return (el) =>
          el.localName !== "script" && el.localName !== "style" && hit(el) &&
          ![...el.children].some((child) => hit(child));
      }
      default:
        throw new Error("unknown query kind: " + by.kind);
    }
  }

  // Every rendered element a query matches, inside its scope when it has one.
  // When the scope itself is not exactly one element: how many it was.
  function find(query) {
    let root = document;
    if (query.within) {
      const scope = find(query.within);
      if (scope.length !== 1) return { scope: scope.length };
      root = scope[0];
    }
    const matches = matcher(query.by);
    const found = [];
    for (const el of walk(root)) {
      if (el !== root && rendered(el) && matches(el)) found.push(el);
    }
    return found;
  }

  const count = (query) => {
    const found = find(query);
    return Array.isArray(found) ? found.length : found.scope;
  };

  // The one element, or how many there were.
  function locate(query) {
    const found = find(query);
    if (!Array.isArray(found)) return found.scope;
    return found.length === 1 ? found[0] : found.length;
  }

  const isPassword = (el) =>
    el.localName === "input" &&
    ((el.getAttribute("type") || "").toLowerCase() === "password" ||
      /password/i.test(el.getAttribute("autocomplete") || ""));

  const checkable = (el) =>
    el.localName === "input" && ["checkbox", "radio"].includes((el.getAttribute("type") || "").toLowerCase());

  // Every password input of the page, for the engine to mask in a snapshot.
  const passwords = () => [...walk(document)].filter(isPassword);

  const stale = (el) => !(el instanceof Element) || !el.isConnected;

  function read(el, what, limit) {
    if (stale(el)) return { stale: true };
    if (isPassword(el)) return { password: true };
    let value;
    switch (what.kind) {
      case "value":
        if ("value" in el) value = String(el.value);
        else if (el.isContentEditable) value = el.textContent;
        else return { absent: true };
        break;
      case "text":
        value = "innerText" in el ? el.innerText : el.textContent;
        break;
      case "checked":
        if (checkable(el)) return { flag: el.checked };
        if (el.hasAttribute("aria-checked")) return { flag: el.getAttribute("aria-checked") === "true" };
        return { absent: true };
      case "selected_text":
        if (el.localName !== "select") return { absent: true };
        value = el.selectedOptions.length ? norm(el.selectedOptions[0].textContent) : "";
        break;
      case "attribute":
        if (!el.hasAttribute(what.name)) return { absent: true };
        value = el.getAttribute(what.name);
        break;
      default:
        throw new Error("unknown read: " + what.kind);
    }
    if (value.length > limit) return { too_large: true };
    return { text: value };
  }

  const disabled = (el) =>
    el.matches(":disabled") || (el.closest("[aria-disabled]") || el).getAttribute("aria-disabled") === "true";

  const editable = (el) =>
    ((el.localName === "input" || el.localName === "textarea") && !el.readOnly) || el.isContentEditable;

  // Whether the element can take the input, and where its middle is. One
  // call: visible, enabled, fit for the input, and the one that would
  // receive an event at its middle.
  function check(el, need) {
    if (stale(el)) return { stale: true };
    if (!rendered(el)) return { no: "not_visible" };
    if (disabled(el)) return { no: "disabled" };
    if (need === "edit" && !editable(el)) return { no: "not_editable" };
    if (need === "select" && el.localName !== "select") return { no: "wrong_control" };
    if (need === "check" && !checkable(el)) return { no: "wrong_control" };
    el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    const box = el.getBoundingClientRect();
    const x = Math.round(box.left + box.width / 2);
    const y = Math.round(box.top + box.height / 2);
    if (need !== "edit" && need !== "select") {
      let hit = el.getRootNode().elementFromPoint(x, y);
      // A label passes its click on to its control.
      const passes = (at) =>
        at === el || el.contains(at) || (at.localName === "label" && at.control === el) ||
        (at.closest && at.closest("label") && at.closest("label").control === el);
      if (!hit || !passes(hit)) return { no: "obscured" };
    }
    return { ok: true, x, y, checked: checkable(el) ? el.checked : null };
  }

  // Focus and select what is there, so that inserted text replaces it.
  function prepareFill(el) {
    const checked = check(el, "edit");
    if (!checked.ok) return checked;
    el.focus();
    if (typeof el.select === "function") el.select();
    else {
      const range = document.createRange();
      range.selectNodeContents(el);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
    return checked;
  }

  const fire = (el, type) => el.dispatchEvent(new Event(type, { bubbles: true, composed: true }));

  // Sets the value through the element's own setter, past a framework's
  // wrapper, and tells the page as a person's edit would.
  function nativeSet(el, text) {
    const checked = check(el, "edit");
    if (!checked.ok) return checked;
    if (el.isContentEditable) el.textContent = text;
    else {
      const proto = el.localName === "textarea" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, text);
    }
    fire(el, "input");
    fire(el, "change");
    return checked;
  }

  function selectByValue(el, value) {
    const checked = check(el, "select");
    if (!checked.ok) return checked;
    const option = [...el.options].find((candidate) => candidate.value === value);
    if (!option || option.disabled) return { no: "no_such_option" };
    if (el.value !== value) {
      el.value = value;
      fire(el, "input");
      fire(el, "change");
    }
    return checked;
  }

  const textMatches = (el, match) => {
    const text = norm("value" in el && typeof el.value === "string" && el.localName !== "button" ? el.value : el.textContent);
    return match.contains ? text.includes(match.text) : text === match.text;
  };

  function holds(condition) {
    switch (condition.kind) {
      case "visible":
        return count(condition.query) >= 1;
      case "hidden":
        return count(condition.query) === 0;
      case "enabled": {
        const found = locate(condition.query);
        return found instanceof Element && !disabled(found);
      }
      case "text": {
        const found = locate(condition.query);
        return found instanceof Element && !isPassword(found) && textMatches(found, condition.match);
      }
      case "url":
        return condition.contains ? location.href.includes(condition.text) : location.href === condition.text;
      case "all":
        return condition.of.every(holds);
      case "any":
        return condition.of.some(holds);
      default:
        throw new Error("unknown condition: " + condition.kind);
    }
  }

  // Resolves true as soon as the condition holds. An observer and the
  // page's own events wake it; nothing asks again on a timer. The timer
  // here only ends the wait, at the engine's deadline, and tidies up.
  function until(condition, timeoutMs) {
    return new Promise((resolve) => {
      let observer = null;
      let timer = null;
      const events = ["input", "change", "load", "hashchange", "popstate", "transitionend", "animationend"];
      const done = (answer) => {
        if (observer) observer.disconnect();
        clearTimeout(timer);
        for (const type of events) removeEventListener(type, look, true);
        resolve(answer);
      };
      function look() {
        let now;
        try {
          now = holds(condition);
        } catch (error) {
          return done({ error: String(error && error.message) });
        }
        if (now) done({ ok: true });
      }
      observer = new MutationObserver(look);
      observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
      for (const type of events) addEventListener(type, look, true);
      timer = setTimeout(() => done({ ok: false }), timeoutMs);
      look();
    });
  }

  // A request from the page's own origin, with the page's own session.
  async function request(spec, limit) {
    const response = await fetch(spec.address, {
      method: spec.method,
      headers: spec.headers,
      body: spec.body === null ? undefined : spec.body,
      credentials: "include",
      redirect: "follow",
    });
    const body = await response.text();
    if (body.length > limit) return { too_large: true };
    const headers = [];
    response.headers.forEach((value, name) => headers.push([name, value]));
    return { status: response.status, address: response.url, headers, body };
  }

  const page = () => ({ address: location.href, title: document.title });

  globalThis.__a2m = { locate, read, check, prepareFill, nativeSet, selectByValue, until, request, page, passwords };
})();
