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
pending hook, the schemas, the eight tools, and the sixth skill workflow.

The rest of the surface is elsewhere in the same document: the plan layer
these kinds share is [§20](SPEC.md#20-operation-plans-and-approval),
migration `0004_operations`, the cache epochs and the pending hook are
[§10](SPEC.md#10-cache-state-and-sync), the admission lock names are
[§9](SPEC.md#9-config-and-paths), the refusal reasons join the exit table in
[§14](SPEC.md#14-errors-and-exit-codes), the `operation.state` event is
[§22](SPEC.md#22-coordinator-events-watch-notify), the MCP tools are
[§21](SPEC.md#21-agent-surface), and `operation@1`,
`operation_reconcile@1`, and the operation block on `plan@1`, `Journal`, and
`receipt@1` are
[Appendix D](SPEC.md#appendix-d-json-result-payloads).

The decisions this file used to list are in those sections, and the open
ones are SPEC §19 items 37, 38, 39, 40, and 45.

Review: [`reviews/code-M8-b.md`](reviews/code-M8-b.md).
