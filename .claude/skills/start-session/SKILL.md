---
name: start-session
description: Begin a working session on Anything2MCP. Use at the start of every session, before planning or writing anything, to pick up where the last session stopped and state what this session will do.
---

# Start a session

An agent begins with no memory of earlier sessions. The repository is the memory. Do these in order and do not skip ahead.

1. Read the newest entry in `docs/journal/` (the table in `docs/journal/README.md` lists them newest first). Its "Next" section is where to begin.
2. Read `docs/plan/working-method.md` if you have not this session. It is short and it governs how work is done here.
3. Find the current slice in `docs/plan/core-first.md`. Read only the spec sections that slice needs; `docs/plan/phase-map.md` says which.
4. Run `git status` and `git log --oneline -10` to see what is actually in the repository, and `python3 tools/docs_audit.py` to confirm the documents are consistent. If the journal and the repository disagree, the repository is right; note the difference.
5. Write down, in a few lines: where the project stands, what you will do this session, which hypotheses it tests, and anything under "For the owner" that touches it. Say it in your first message; it also opens the session's journal entry.
6. Begin. Do not wait for a go-ahead: agents here plan, build, and merge on their own (`AGENTS.md`, "How agents work here"). If something there is the owner's to settle, choose other work and leave a Proposed decision record.

Keep the steps small. One slice per session is a good pace for one agent. More work means more agents on separate packages, each with its own branch, never a bigger step.
