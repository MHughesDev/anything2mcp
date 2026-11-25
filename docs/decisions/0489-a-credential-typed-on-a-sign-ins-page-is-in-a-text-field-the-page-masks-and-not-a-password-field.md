# 0489. A credential typed on a sign-in's page is in a text field the page masks, and not a password field

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Builds** package SR5, from finding SR-3 of the [security review of 2026-11-25](../reviews/security-review-2026-11-25.md) ([decision 0484](0484-a-release-is-gated-by-its-environments-and-a-person-approves-every-publish.md)). It **changes** how the act page of a sign-in ([decision 0205](0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)) serves each credential's field, in `a2m-local`; spec section [503](../spec/5-security/503-approval-and-sign-in-surfaces.md); the design of [`a2m-local`](../architecture/crates/a2m-local.md); the roadmap (SR5 done). It **keeps** everything else the page does: the Host and Origin checks, the one-time value of each press, the policy that lets no script run and no other page frame it, `no-store`, the field served empty and never filled again, and the credential carried with the press to the engine alone, which seals it in the vault for the person (rule 4).

## Context

A sign-in that takes a credential, such as the web pack's `form_login`, asks the person for it on the engine's page, in their own browser. The field was of type `password` with `autocomplete="off"`. Browsers ignore `autocomplete="off"` on a password field and offer to save what was typed, under the listener's address, in their own password store and its sync: a copy outside the vault, which the engine cannot see, seal, or remove. The page cannot stop a browser doing what it does with a password field; it can only not serve one.

## Decision

1. **No field of type `password`.** Each credential the page asks for is an `<input type="text">`, so that no browser takes it for a password to offer to keep. Its style, `-webkit-text-security: disc`, inline in the page as the page's policy already allows, shows dots in place of what is typed where the browser applies it.
2. **Nothing remembered and nothing sent.** The field has `autocomplete="off"` (which, on a text field, browsers are documented to honour for the history of values typed into forms; not verified here in every browser), `autocapitalize="off"`, `autocorrect="off"`, and `spellcheck="false"`, so that what is typed is not kept as form history, not changed, and not sent to a spelling service the browser may use. The form of Carry out also has `autocomplete="off"`.
3. **The page says the rest.** Beside each field: "If your browser offers to save or remember what you typed here, decline: it belongs in the engine's vault only." Above them, that the browser may show what is typed in the clear, so to type it where nobody can read the screen.
4. **The one-time code's page stays as it is.** `pages/code.html` keeps its field of type `password` with `autocomplete="one-time-code"`: a code is good once, so a copy a browser keeps unlocks nothing, and a browser that knows the field as a one-time code fills it from a message where it can.

The proof is `the_sign_in_page_offers_no_password_field_for_the_browser_to_keep` in `crates/a2m-local/src/tests/surface.rs`: the page of a provider that takes a username and a password, as `form_login` does, has no `type="password"` and no script, serves each field with exactly the attributes above and the sentence beside it, carries the style and the form's `autocomplete="off"`, and keeps its headers. The test is in `a2m-local`, which names no pack, so its provider is the crate's own test provider of a username and a password and not the web pack's `form_login`; the page is the same for every provider.

## What it costs the person

- **Where a browser does not apply the style, what is typed shows in the clear.** Chromium-based browsers and Safari apply `-webkit-text-security`; whether every version of Firefox does is not verified here. The page says so, on one person's machine, where the risk is someone reading the screen.
- **The person's own password manager may not offer to fill the field,** since it is not a password field. The person can still paste from it. The browser's offer to keep it was the harm, and losing the offer to fill is the price.
- **A screen reader announces a text field,** and may read back what is typed. A person who uses one hears their credential spoken; the page's words say what the field is for.

## What was rejected

- **Taking each credential in a window of the engine's own browser, in a profile that keeps no passwords.** The fallback section 503 names and the default the roadmap gave. Not taken now: that window is the web pack's ([decision 0254](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)), while the act page is the kernel's and serves every pack's sign-in, including an HTTP service's or a data store's that never starts a browser; it would start a browser for each sign-in, shows a window only on a desktop (proven on Linux on a virtual display only), and moves the page's Host, Origin, and one-time protections into a second surface to be built and tested. It is the stronger answer for a web application and stays open as a later row if this one shows its limits.
- **A form that is never submitted as a form,** sending the value by script. The pages run no script by their policy (`default-src 'none'`), and a policy that let one run would be a larger loss than the one it fixes.
- **`autocomplete="new-password"` or `"one-time-code"` on a password field.** Both still name a password field; the first asks the browser to offer a new password to save, the second is honoured as a hint only, and neither stops the offer to keep.

## What is still not in our hands

- A browser may change how it decides what is a password. A browser that someday offers to keep a masked text field, or an extension the person installed that reads every field, is outside the page.
- Autofill of a text field from data the browser already holds, against `autocomplete="off"`, is the browser's choice; it fills, it does not keep.
- Whatever the person's machine records of what is typed (a keyboard program, an input method that learns words) is outside the product, as it is for every program on that machine.

## What would reopen it

A browser that offers to keep, or sends elsewhere, what is typed into a field served this way; or a person who cannot use the field, by sight or by a reader. Either moves the credential to the engine's own window, as above.
