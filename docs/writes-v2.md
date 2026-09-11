# Writes v2 — pointer

This document recorded the M8-b write contract — discussion replies, the
two inbox writes, and the operation journal behind them — while
`docs/SPEC.md` did not carry it.

**It is now [`SPEC.md` §25](SPEC.md#25-discussion-and-inbox-writes).** That
section holds the contract table with every request and admission lock, the
prepare reads and the readbacks, the three plan kinds and what they freeze,
the body transforms, the refusal table, the operation journal and its
states, the recovery table, the ambiguous-outcome rules and
`--assume-not-posted`, the attribution ladder and the "accepted is not
delivered" rule, the response allowlist, the exit table, receipts and the
pending hook, the schemas, and the sixth skill workflow.

**These writes are CLI-only, and so is everything else.** This file once
named eight MCP tools for them. There are none, and there is no MCP tool for
a read either: the owner narrowed the `canvas mcp` catalog to reads on
2026-09-10 and then cut it to a single `getclitools` tool the same day. That
one tool performs nothing — it returns the `canvas` command reference and the
agent runs the commands itself. The commands — `canvas discussion reply`,
`canvas inbox send`, `canvas inbox reply`, `canvas operation
status|reconcile` — are unchanged, and each asks for its approval at the
terminal. The one-tool surface is [§21](SPEC.md#21-agent-surface); what the
writes themselves do is [§25](SPEC.md#25-discussion-and-inbox-writes); the
decisions are SPEC §19 items 48 and 50.

`canvas schema` now describes these three writes under their own names —
`canvas schema "discussion reply"`, `"inbox send"`, `"inbox reply"` — each
through the `operation@1` entry that owns the shape they print.

The rest of the surface is elsewhere in the same document: the plan layer
these kinds share is [§20](SPEC.md#20-operation-plans-and-approval),
migration `0004_operations`, the cache epochs and the pending hook are
[§10](SPEC.md#10-cache-state-and-sync), the admission lock names are
[§9](SPEC.md#9-config-and-paths), the refusal reasons join the exit table in
[§14](SPEC.md#14-errors-and-exit-codes), the `operation.state` event is
[§22](SPEC.md#22-coordinator-events-watch-notify), and `operation@1`,
`operation_reconcile@1`, and the operation block on `plan@1`, `Journal`, and
`receipt@1` are
[Appendix D](SPEC.md#appendix-d-json-result-payloads).

The decisions this file used to list are in those sections, and the open
ones are SPEC §19 items 37, 39, 40, and 45. Item 38 is moot: it asked about
an MCP annotation on a tool that no longer exists, and there is no annotation
system left to ask it of.

Review: [`reviews/code-M8-b.md`](reviews/code-M8-b.md).
