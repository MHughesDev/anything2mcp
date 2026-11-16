---
name: start-session
description: Begin a working session on Web2MCP. Use at the start of every session, before planning or writing anything, to pick up where the last session stopped.
---

# Start a session

An agent begins with no memory of earlier sessions. The repository is the memory. Do these in order and do not skip ahead.

1. Read the newest entry in `docs/journal/` (the table in `docs/journal/README.md` lists them newest first). Its "Next" section is where to begin.
2. Read `docs/plan/working-method.md` if you have not this session. It is short and it governs how work is done here.
3. Find the current slice in `docs/plan/core-first.md`. Read only the spec sections that slice needs; `docs/plan/phase-map.md` says which.
4. Run `git status` and `git log --oneline -10` to see what is actually in the repository, and `python3 tools/docs_audit.py` to confirm the documents are consistent. If the journal and the repository disagree, the repository is right; note the difference.
5. Tell the owner, in a few lines: where the project stands, what you propose to do this session, which hypotheses it tests, and anything in "Undecided" that blocks it.
6. Wait for the owner's go-ahead before creating or changing files.

Keep the pace slow. One slice per session is good. If the proposed work is more than one slice, propose less.
