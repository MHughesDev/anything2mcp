---
name: research-brief
description: Run one open research brief for Web2MCP and fold the result into the design. Use before a phase or slice that depends on a brief in docs/research/11-open-research.md, or when a design question needs outside facts.
argument-hint: "[brief number]"
---

# Run a research brief

Most of the existing research came from search-result summaries and is marked Partial. This is how a brief gets done properly.

1. **Read the brief** in `docs/research/11-open-research.md` and the spec sections it feeds.
2. **Go to primary sources.** Official documentation, specifications, source repositories, and release notes. Open them and read them; do not rely on a summary of them. Note the date of anything that changes quickly.
3. **Check hands-on where it is cheap.** If a claim can be confirmed by running something, such as a crate building on three operating systems, run it.
4. **Write the note** as the next numbered file in `docs/research/`: what was found, each claim with its source, what it means for the design, and a closing list of what could not be verified. Mark its status honestly.
5. **Update the design.** Correct the spec sections the brief feeds. Add a decision record if a decision changes. Add or settle hypotheses.
6. **Update the indexes:** add the note to `docs/research/README.md`, remove or mark the brief in `docs/research/11-open-research.md`, and run `python3 tools/docs_audit.py --write-sources` so the source index is rebuilt.
7. **Report** what changed in the design because of it, and what stayed uncertain.

Do not quote long passages from sources; state findings in your own words with a link. Record what a source says and how reliable it is, including when a number comes from the project that benefits from it.
