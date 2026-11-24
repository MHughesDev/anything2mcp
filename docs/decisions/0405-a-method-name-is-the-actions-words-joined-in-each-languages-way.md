# 0405. A method name is the action's words, joined in each language's way

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Built in `bindings/python/tools/generate_actions.py` (`method_name`), `bindings/npm/scripts/generate-actions.js` (`methodName`), and `bindings/python/tools/fetch_openapi.py` (`--check`).

**Date:** 2026-11-24

**Changes:** how the two clients name a generated method ([decision 0156](0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md)); what CI compares the saved OpenAPI document with.

## Context

[Decision 0156](0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md) names a method as its action names it: `tool.call` is `tool_call` in Python and `toolCall` in Node. Its rule for Node split the name only at the dot, so an action whose noun or verb has an underscore, `policy.tools_allowed` and `policy.who_can`, had no name the Node generator would take, and regenerating stopped. The saved document, `bindings/python/tools/openapi.json`, had not been refreshed since those actions were added, and nothing compared it with the document the engine serves, so the clients lacked every action added after it was saved.

## Decision

1. **An action's name has one form:** a noun and a verb, each one or more words of lower-case letters and digits joined by single underscores. Both generators check it and stop on any other, naming the action.
2. **Python joins the words with underscores:** the dot becomes one, and an underscore stays. `policy.tools_allowed` is `policy_tools_allowed`.
3. **Node joins the same words in camel case,** splitting at the dot and at each underscore. `policy.tools_allowed` is `policyToolsAllowed`. An action named as before keeps its method name in both.
4. **Two actions with one method name stop the generator,** as before; neither picks one.
5. **The saved document is checked against the engine.** `fetch_openapi.py --binary <program> --check` starts an engine of the built program in a scratch home, takes the document it serves, and fails when the saved copy differs. CI runs it in the bindings job beside the generators' `--check`, on Linux and macOS, so the saved copy cannot drift from the product unnoticed.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the underscore in Node (`policyTools_allowed`) | Neither the language's way nor the action's; a reader cannot tell where a word ends |
| Carry a method name per action in the document, from `a2m-actions` | A change to the registry and the served document for what one rule gives; worth it only if a name ever needs choosing by hand |
| Refresh the saved document by hand at each release, with no check | It is how the copy drifted |
| Compare only on Linux, or on all three systems | A real engine is started by the clients' tests on Linux and macOS only; Windows is left out until those tests run there, and the document is made from the same registry on every system |

## Revisit if

An action needs a method name the rule does not give, or the clients' real-engine tests run on Windows.
