---
name: end-session
description: Close a working session on Web2MCP. Use before stopping work for any reason, so the next session can continue. Writes the journal entry and checks the repository is left in a true state.
---

# End a session

A session that ends without a journal entry has lost its work for the next agent.

1. Run the tests and lints that exist, and `python3 tools/docs_audit.py`. Fix what fails, or record plainly that it fails.
2. Check that the documents are true: any section the session's work contradicted is corrected, any decision made has a record in `docs/decisions/`, any hypothesis tested has a result file in `docs/hypotheses/` and its status updated in the register.
3. Write a journal entry in `docs/journal/` using the layout in `docs/journal/README.md`, and add it to the top of the table there. Be exact about what is finished, what is half-done, and what failed.
4. Make sure "Next" names one concrete first step.
5. Run `git status`. Tell the owner what is uncommitted and ask whether to commit and push. Do not push without being asked.
6. Give the owner a short report: what works now, what was learned about the design, what changed in the documents, and what you would do next.

Never write that something works unless a test run in this session showed it.
