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

  // The origins this tab was opened for. The engine writes the list in
  // when it places the helper; as the file stands, the list is empty and
  // every call is refused.
  const ALLOWED = /*a2m:origins*/[];

  // Where the page is: its origin, or its scheme when it has none.
  const here = () => (location.origin !== "null" ? location.origin : location.protocol);

  // Every function the engine calls goes through this, in the same call
  // as the look or the act itself: on a page of an origin the tab was not
  // opened for, nothing is located, read, checked, focused, or sent.
  const guarded = (fn) =>
    function (...args) {
      const at = here();
      if (!ALLOWED.includes(at)) throw new Error("a2m-outside-origins " + at);
      return fn.apply(this, args);
    };

  const norm = (text) => (text || "").replace(/\s+/g, " ").trim();

  // Elements a person can perceive: laid out, and not hidden.
  const rendered = (el) =>
    el.isConnected &&
    el.getClientRects().length > 0 &&
    el.ownerDocument.defaultView.getComputedStyle(el).visibility !== "hidden";

  const hiddenFromTree = (el) => {
    for (let at = el; at; at = at.parentElement || (at.getRootNode() && at.getRootNode().host)) {
      if (at.getAttribute && at.getAttribute("aria-hidden") === "true") return true;
    }
    return false;
  };

  // Every element under a root, in document order, through open shadow roots.
  function* walk(root) {
    const start = root.nodeType === Node.ELEMENT_NODE ? root : null;
    // The document of the root, which is another one inside a frame.
    const owner = root.ownerDocument || root;
    const walker = owner.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
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
    summary: "button", hr: "separator", p: "paragraph", dt: "term", dd: "definition",
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

  // The cells of a row, in order: its own, not those of a row inside it.
  const CELL_ROLES = new Set(["cell", "gridcell", "columnheader", "rowheader"]);
  const cellsOf = (row) => [...walk(row)].filter((el) => el !== row && CELL_ROLES.has(roleOf(el)));

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
        // A selector on the `value` attribute would tell, match by match,
        // what a password input was served with. On a page that holds one,
        // such a selector is refused whole.
        if (asksValue(by.selector) && [...walk(document)].some((el) => isPassword(el) && el.hasAttribute("value"))) {
          throw new Error("a2m-password-refused");
        }
        return (el) => el.matches(by.selector);
      case "row_keyed":
        return (el) => {
          if (roleOf(el) !== "row" || hiddenFromTree(el)) return false;
          const first = cellsOf(el)[0];
          return !!first && norm(first.textContent) === by.key;
        };
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
      // A frame is a scope that is entered: its own document, when it is of
      // the page's own origin. A frame of another origin has none to enter,
      // and is no scope at all.
      if (root.localName === "iframe" || root.localName === "frame") {
        if (!root.contentDocument) return { scope: 0 };
        root = root.contentDocument;
      }
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

  // Whether a selector asks about the `value` attribute. A selector with an
  // escape in it could spell the name another way, so it counts as asking.
  const asksValue = (selector) => /\\|\[\s*(?:[*\w-]*\|)?\s*value/i.test(selector);

  const isPassword = (el) =>
    el.localName === "input" &&
    ((el.getAttribute("type") || "").toLowerCase() === "password" ||
      /password/i.test(el.getAttribute("autocomplete") || ""));

  const checkable = (el) =>
    el.localName === "input" && ["checkbox", "radio"].includes((el.getAttribute("type") || "").toLowerCase());

  // Every password input of the page, for the engine to mask in a snapshot.
  const passwords = () => [...walk(document)].filter(isPassword);

  // An element of a frame is of that frame's own `Element`, so it is known
  // by its kind of node and not by the page's own class.
  const isElement = (el) => !!el && el.nodeType === Node.ELEMENT_NODE;
  const stale = (el) => !isElement(el) || !el.isConnected;

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
      case "cell": {
        const cell = cellsOf(el)[what.index];
        if (!cell) return { absent: true };
        value = norm(cell.textContent);
        break;
      }
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
    let x = Math.round(box.left + box.width / 2);
    let y = Math.round(box.top + box.height / 2);
    if (need !== "edit" && need !== "select") {
      let hit = el.getRootNode().elementFromPoint(x, y);
      // A label passes its click on to its control.
      const passes = (at) =>
        at === el || el.contains(at) || (at.localName === "label" && at.control === el) ||
        (at.closest && at.closest("label") && at.closest("label").control === el);
      if (!hit || !passes(hit)) return { no: "obscured" };
    }
    // The middle is in the coordinates of the page the person sees: each
    // frame the element is in moves it by where the frame is.
    for (let view = el.ownerDocument.defaultView; view && view.frameElement; view = view.parent) {
      const frame = view.frameElement.getBoundingClientRect();
      x += Math.round(frame.left + view.frameElement.clientLeft);
      y += Math.round(frame.top + view.frameElement.clientTop);
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
      const range = el.ownerDocument.createRange();
      range.selectNodeContents(el);
      const selection = el.ownerDocument.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
    return checked;
  }

  const fire = (el, type) =>
    el.dispatchEvent(new (el.ownerDocument.defaultView.Event)(type, { bubbles: true, composed: true }));

  // Sets the value through the element's own setter, past a framework's
  // wrapper, and tells the page as a person's edit would.
  function nativeSet(el, text) {
    const checked = check(el, "edit");
    if (!checked.ok) return checked;
    if (el.isContentEditable) el.textContent = text;
    else {
      const view = el.ownerDocument.defaultView;
      const proto = el.localName === "textarea" ? view.HTMLTextAreaElement.prototype : view.HTMLInputElement.prototype;
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
        return isElement(found) && !disabled(found);
      }
      case "text": {
        const found = locate(condition.query);
        return isElement(found) && !isPassword(found) && textMatches(found, condition.match);
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
      const watching = new Set();
      const frames = [];
      const done = (answer) => {
        if (observer) observer.disconnect();
        clearTimeout(timer);
        for (const type of events) removeEventListener(type, look, true);
        for (const frame of frames) frame.removeEventListener("load", look);
        resolve(answer);
      };
      // A frame of the page's own origin changes without the page's own
      // document changing: its document is watched too, as it appears.
      function watchFrames(root) {
        for (const el of walk(root)) {
          if ((el.localName === "iframe" || el.localName === "frame") && !watching.has(el)) {
            watching.add(el);
            frames.push(el);
            el.addEventListener("load", look);
          }
          if ((el.localName === "iframe" || el.localName === "frame") && el.contentDocument) {
            if (!watching.has(el.contentDocument)) {
              watching.add(el.contentDocument);
              observer.observe(el.contentDocument, { subtree: true, childList: true, attributes: true, characterData: true });
            }
            watchFrames(el.contentDocument);
          }
        }
      }
      function look() {
        let now;
        watchFrames(document);
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
      // A redirect is not followed: its next address was checked by nobody.
      redirect: "manual",
    });
    if (response.type === "opaqueredirect") return { redirected: true };
    const body = await response.text();
    if (body.length > limit) return { too_large: true };
    const headers = [];
    response.headers.forEach((value, name) => headers.push([name, value]));
    return { status: response.status, address: response.url, headers, body };
  }

  // A read from the page's own origin, with the page's own session: GET,
  // to a path the engine made and checked, with no header and no body. A
  // redirect is not followed. The answer is read as text up to the limit,
  // and the rest is not read at all.
  async function readFromPage(path, limit) {
    // A page with no origin of its own, such as a `data:` page, has no
    // session to read with.
    if (location.origin === "null") throw new Error("a2m-path-refused");
    if (typeof path !== "string" || !path.startsWith("/") || path.startsWith("//")) {
      throw new Error("a2m-path-refused");
    }
    const address = new URL(path, location.origin);
    if (address.origin !== location.origin) throw new Error("a2m-path-refused");
    const response = await fetch(address.href, {
      method: "GET",
      credentials: "same-origin",
      redirect: "manual",
      cache: "no-store",
    });
    if (response.type === "opaqueredirect") return { redirected: true };
    const type = (response.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
    let body = "";
    if (response.body) {
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        body += decoder.decode(value, { stream: true });
        if (body.length > limit) {
          await reader.cancel();
          return { too_large: true };
        }
      }
      body += decoder.decode();
      if (body.length > limit) return { too_large: true };
    }
    return { status: response.status, media_type: type, body };
  }

  // The page's own anti-forgery token, from the place the entry names:
  // the one text the page holds there, or why there is none to send.
  const TOKEN_FORM = /^[A-Za-z0-9\-_.~+\/=]{8,512}$/;
  function pageToken(from, name) {
    const found = new Set();
    if (from === "meta") {
      for (const el of document.querySelectorAll("meta")) {
        if (el.getAttribute("name") === name) found.add(el.getAttribute("content") || "");
      }
    } else if (from === "input") {
      for (const el of document.querySelectorAll("input")) {
        if (el.type === "hidden" && el.name === name) found.add(el.value);
      }
    } else if (from === "cookie") {
      for (const pair of document.cookie.split(";")) {
        const at = pair.indexOf("=");
        if (at > 0 && pair.slice(0, at).trim() === name) found.add(pair.slice(at + 1).trim());
      }
    } else {
      return { problem: "unusable" };
    }
    if (found.size === 0) return { problem: "missing" };
    if (found.size > 1) return { problem: "several" };
    const [value] = found;
    if (!TOKEN_FORM.test(value)) return { problem: "unusable" };
    return { value };
  }

  // A write from the page's own origin, with the page's own session and
  // the page's own anti-forgery token, read here, in the same call that
  // sends it. The token is never handed back: every copy of it in the
  // answer is replaced before the answer leaves the page, and no message of
  // an error is handed back either, since one could quote what was sent.
  // Everything before `fetch` is checked first; an answer of `refused`
  // means nothing was sent, and `failed` that whether it was is not known.
  async function writeFromPage(spec, limit) {
    if (location.origin === "null") return { refused: "path" };
    const path = spec.path;
    if (typeof path !== "string" || !path.startsWith("/") || path.startsWith("//")) {
      return { refused: "path" };
    }
    const address = new URL(path, location.origin);
    if (address.origin !== location.origin) return { refused: "path" };
    if (!["POST", "PUT", "PATCH", "DELETE"].includes(spec.method)) return { refused: "method" };
    const fields = {};
    for (const [name, value] of Object.entries(spec.fields || {})) fields[name] = value;
    const headers = {};
    let token = null;
    if (spec.token) {
      const found = pageToken(spec.token.from, spec.token.name);
      if (found.problem) return { refused: "token_" + found.problem };
      token = found.value;
      if (spec.token.to === "header") headers[spec.token.to_name] = token;
      else if (spec.token.to === "field" && !(spec.token.to_name in fields)) fields[spec.token.to_name] = token;
      else return { refused: "token_place" };
    }
    let body;
    if (spec.encoding === "json") {
      headers["Content-Type"] = "application/json";
      body = JSON.stringify(fields);
    } else if (spec.encoding === "form") {
      headers["Content-Type"] = "application/x-www-form-urlencoded";
      body = new URLSearchParams(Object.entries(fields).map(([name, value]) => [name, String(value)])).toString();
    } else {
      return { refused: "encoding" };
    }
    const scrub = (text) => {
      if (token === null) return text;
      let out = text;
      for (const form of new Set([token, token.split("/").join("\\/"), encodeURIComponent(token)])) {
        out = out.split(form).join("[redacted]");
      }
      return out;
    };
    let response;
    try {
      response = await fetch(address.href, {
        method: spec.method,
        headers,
        body,
        credentials: "same-origin",
        redirect: "manual",
        cache: "no-store",
      });
    } catch (_) {
      return { failed: true };
    }
    try {
      if (response.type === "opaqueredirect") return { redirected: true };
      const type = (response.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
      let text = "";
      if (response.body) {
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          text += decoder.decode(value, { stream: true });
          if (text.length > limit) {
            await reader.cancel();
            return { too_large: true };
          }
        }
        text += decoder.decode();
        if (text.length > limit) return { too_large: true };
      }
      // An answer that quotes the token in any spelling is withheld whole:
      // the scrub above knows three spellings, and an application can echo
      // it in others (decision 0245, after audit finding S4).
      if (token !== null && echoes(text, token)) {
        return { status: response.status, media_type: scrub(type), body: "", withheld: true };
      }
      return { status: response.status, media_type: scrub(type), body: scrub(text) };
    } catch (_) {
      return { failed: true };
    }
  }

  // Whether `text` holds `token`, or its first half, in any spelling that
  // JSON escapes, HTML character references, percent-encoding in either
  // case, or a change of case can make of it, alone or after one another.
  function echoes(text, token) {
    const point = (code) => {
      try {
        return String.fromCodePoint(code);
      } catch (_) {
        return "";
      }
    };
    const json = (s) =>
      s.replace(/\\u([0-9a-fA-F]{4})/g, (_, h) => point(parseInt(h, 16))).replace(/\\(.)/g, "$1");
    const entities = (s) =>
      s
        .replace(/&#x([0-9a-fA-F]{1,6});?/g, (_, h) => point(parseInt(h, 16)))
        .replace(/&#([0-9]{1,7});?/g, (_, d) => point(parseInt(d, 10)))
        .replace(/&(amp|quot|apos|lt|gt|sol|plus|equals|tilde|num|percnt);/gi, (_, n) =>
          ({ amp: "&", quot: '"', apos: "'", lt: "<", gt: ">", sol: "/", plus: "+", equals: "=", tilde: "~", num: "#", percnt: "%" })[n.toLowerCase()]
        );
    const percent = (s) => s.replace(/%([0-9a-fA-F]{2})/g, (_, h) => point(parseInt(h, 16)));
    const lower = token.toLowerCase();
    const half = lower.slice(0, Math.max(8, Math.ceil(lower.length / 2)));
    let forms = [text];
    for (let round = 0; round < 3; round += 1) {
      const next = [];
      for (const form of forms) next.push(json(form), entities(form), percent(form));
      forms = forms.concat(next);
      if (forms.length > 200) break;
    }
    return forms.some((form) => {
      const seen = form.toLowerCase();
      return seen.includes(lower) || seen.includes(half);
    });
  }

  const page = () => ({ address: location.href, title: document.title });

  const all = { locate, read, check, prepareFill, nativeSet, selectByValue, until, request, readFromPage, writeFromPage, page, passwords };
  globalThis.__a2m = Object.freeze(
    Object.fromEntries(Object.entries(all).map(([name, fn]) => [name, guarded(fn)]))
  );
})();
