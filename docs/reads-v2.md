# Reads v2 — pointer

This document recorded the M8-a read contract — pages, the syllabus,
discussions, and the inbox — while `docs/SPEC.md` did not carry it.

**It is now [`SPEC.md` §23](SPEC.md#23-richer-reads).** That section holds the
commands and their requests, the datasets and TTLs, the reading rules, the
body and reference rules, the exit table, the cache migration and config keys,
and the schemas. The `inbox.unread_count` event the same dataset produces is
[§22](SPEC.md#22-coordinator-events-watch-notify).

**These reads are CLI-only, like every other command.** This file once named
eight MCP read tools for them. There are none, and there is no MCP tool for
any other action either: the owner cut `canvas mcp` to a single
`getclitools` tool on 2026-09-10, and that tool only describes the command
line. `canvas pages`, `canvas page`, `canvas syllabus`, `canvas discussions`,
`canvas discussion`, `canvas inbox`, `canvas inbox show`, and
`canvas inbox unread-count` are unchanged, and each prints the same §7
envelope with `--json` that it always did. The one-tool surface is
[§21](SPEC.md#21-agent-surface); the decision is SPEC §19 item 50.

The decisions this file used to list are in those sections, and the open ones
are SPEC §19 items 26, 27, 28, and 35.

Reviews: [`reviews/code-M8-a.md`](reviews/code-M8-a.md),
[`reviews/code-M8-a2.md`](reviews/code-M8-a2.md),
[`reviews/code-M8-a3.md`](reviews/code-M8-a3.md).
