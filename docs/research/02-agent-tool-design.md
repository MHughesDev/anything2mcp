# Research: designing tools for AI agents

**Status:** completed by a research agent on 2026-11-15. Feeds [spec section 21](../spec/21-agent-facing-tool-surface.md).

## Question

How should generated tools be named, described, and shaped so that an agent with no prior knowledge of the app uses them correctly?

## Findings

1. **Build tools around workflows, not endpoints.** Anthropic's example: one `schedule_event` replaces `list_users`, `list_events`, and `create_event`. Overlapping tools distract the agent. [Anthropic](https://www.anthropic.com/engineering/writing-tools-for-agents). OpenAI says the same: merge functions always called in sequence, and don't make the model supply arguments your code already knows. [OpenAI](https://developers.openai.com/api/docs/guides/function-calling)
2. **Naming.** Use a service or resource prefix, such as `asana_projects_search`. Anthropic found prefix versus suffix had non-trivial, model-dependent effects but published no numbers. The MCP spec (2026-07-28) allows 1 to 128 characters from `A-Za-z0-9_-.` and says to list tools in a fixed order. The Claude API pattern has no dot: `^[a-zA-Z0-9_-]{1,128}$`. [MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)
3. **Descriptions** are by far the most important factor. Write at least three to four sentences: what it does, when to use it and when not, what each parameter means, and limits. [Claude docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)
4. **Most real descriptions are weak.** A study of 856 tools found 97.1% had at least one defect. Fixing them raised median task success by 5.85 points, but also raised steps by 67% and made 16.7% of cases worse. [arXiv 2602.14878](https://arxiv.org/abs/2602.14878)
5. **Schemas.** Use clear parameter names such as `user_id`, enums, and strict mode. OpenAI advises keeping arguments flat and treats under about 20 arguments as normal. [OpenAI cookbook](https://developers.openai.com/cookbook/examples/o-series/o3o4-mini_prompting_guide). Adding example inputs raised accuracy on complex parameters from 72% to 90%. [Anthropic](https://www.anthropic.com/engineering/advanced-tool-use)
6. **Tool count.** Claude picks tools less accurately beyond 30 to 50. OpenAI's soft target is under 20 tools at turn start. RAG-MCP: success was above 90% with fewer than about 30 candidates, and failures dominated past about 100. [arXiv 2505.03275](https://arxiv.org/abs/2505.03275)
7. **Tool search.** Keep three to five tools always loaded and defer the rest. This raised Opus 4.5 from 79.5% to 88.1% and cut tokens by 85%. Search matches tool names, descriptions, and argument names and descriptions. [Claude tool search](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool). OpenAI advises namespaces of fewer than 10 functions. [OpenAI tool search](https://developers.openai.com/api/docs/guides/tools-tool-search)
8. **Retrieval is the weak link.** Standard search models do poorly at finding tools ([arXiv 2503.01763](https://arxiv.org/abs/2503.01763)). In LiveMCPBench, missed retrievals were the most common failure, often from wording mismatches such as "convert to MP3" versus "extract audio" ([arXiv 2508.01780](https://arxiv.org/abs/2508.01780)).
9. **Responses.** Return names rather than UUIDs. Offer concise and detailed response formats; concise used about a third of the tokens. Use pagination and truncation with sensible defaults; Claude Code caps tool responses at 25,000 tokens.
10. **Errors and state.** Return fixable errors with `isError: true` and actionable feedback. Track state with explicit handles, state their lifetime in the description, and return an error that says when a handle has expired.
11. **Code execution.** Present tools as files, or as a `search_tools` tool with a detail-level parameter. One workflow dropped from 150,000 to 2,000 tokens. [Anthropic](https://www.anthropic.com/engineering/code-execution-with-mcp)
12. **Evals.** Use realistic tasks that need many calls, each with a verifier. Track accuracy, runtime, call count, tokens, and errors. Read the transcripts and keep held-out test sets.

## Recommendations adopted in the spec

1. **Names:** `{target}_{module}_{object}_{verb}`, lowercase with underscores, at most about 60 characters so a client prefix still fits. Name pipelines by outcome. Fail the build on collisions or near-duplicates.
2. **Descriptions:** generated from a required template: purpose in the user's words; when to use it and which pipeline to prefer instead; preconditions; side effects and annotations; what it returns; limits.
3. **Schemas:** flat, about 10 parameters or fewer, enums from declared options, defaults, `additionalProperties: false`, and one to three examples taken from recorded runs. The server fills in session and context values itself.
4. **What the agent sees first:** pipelines and control tools up front, under about 20. Single operations are deferred behind a search tool.
5. **Results:** structured content with an output schema plus a one-line text summary. Concise by default, cursor pagination, a size cap with a note when truncated. Errors name the exact next tool and arguments.

## Still uncertain

- Whether prefix or suffix naming works better; the effect depends on the model.
- Whether MCP clients defer tools consistently. MCP has no standard for tool search or grouping.
- How long descriptions should be, given the gains versus the extra steps and regressions in finding 4.
- The thresholds in finding 6 are soft and model-specific.
- One tool per operation versus a single generic call tool: the generic tool scales better but loses per-operation schema validation on the client side. The spec uses both.
