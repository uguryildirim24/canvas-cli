# Turn 07 — fable review of REPORT.md

Corrections only. Each item names the section, the problem, and the fix. Nothing here changes the agreed design.

**C1. §3.4, §3.6 — paths are missing.** The report will be folded into the spec, whose §9 table lists every file the CLI creates. Add the agreed paths: broker socket `<data root>/bridge/<identity-key>.sock` in `<data root>/bridge/` (mode `0700`, socket `0600`); Windows named pipe `\\.\pipe\canvas-cli-<identity-key>`; request permits `<identity dir>/locks/api-slot-<n>.lock` for `n < api_concurrency`; refresh single-flight `<identity dir>/locks/refresh-<dataset>-<scope>.lock`; the `governor`, `plans`, `approval_handles`, and `events` tables in `state.sqlite`. Say that all of them follow §10's rules (never deleted except by `identity remove`, `fs4` locks).

**C2. §3.5 — the audit field is unnamed.** Turns 03 and 05 agreed that the journal and receipt record how approval was obtained. Add to the `plan@1` and journal rows: `approval { channel: "tty" | "elicitation" | "panel" | "yes-flag", at: ts, consumer?: string, plan_sha256 }`, and add `plan_id?` and `approval?` to the `Journal` and `receipt@1` objects so Appendix D can carry them.

**C3. §3.2, §3.5 — no §14 exit-code mapping.** New domain outcomes need codes so the single-invocation rule holds for the CLI forms. Proposed: plan `expired`, `invalidated`, and `approval_required` → outcome `refused`, exit 8; `here` with no attachment or a paused one → outcome `refused`, exit 8 with `reason: not_attached | paused | validating`; `context.follow` with a stale generation → exit 8; broker not running → outcome `refused`, exit 8 with `reason: bridge_unavailable` (keep 13 for lock and DB failures); `watch` cursor expired → a `resync_required` event, exit 0.

**C4. §3.2 — schema names for the new CLI commands.** Name them so the registry owner can add them: `here@1` (result = `ContextBundle@1`), `bridge@1` (`install|status|detach`), `plan@1` (`submission.prepare` and the human `submit` plan phase), `event@1` (one document per `--jsonl` line), `schema@1` (`canvas schema`, a raw-output exception like `completions`, exit 2 with `--json`).

**C5. §4 — table shape differs from §18.** §18 uses `# | Package | Owns | Depends on | Acceptance`. Split the "Owns / depends on" column into two so the coordinator can paste rows. Also add the round and shared-file owners line: M6-a and M6-b touch the command enum, schema registry, and migration list; M7-a adds an `extension/` directory and the native manifest template; name one owner per shared file per round as §18 does.

**C6. §4 M6-c — dependency on M4-b.** `watch` compares announcements and calendar windows, which M4-b creates. State the dependency as "M6-b, M4-b" for completeness, even though all M5 packages are prerequisites.

**C7. §2 — flag spelling.** The Chrome DevTools MCP configuration document lists the option as `--auto-connect`; the blog post uses `autoConnect`. Cite the configuration document (`https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/configuration.md`) and use its spelling.

**C8. §5 — open questions are not listed as questions.** The brief asks for open questions for Rolf. Add a numbered list at the end of §5: (1) What is the final authenticated origin in your Canvas tab, `courses.example.test` or `courses.lasell.edu`? (2) Which agent do you use most (Claude Code, Codex, Cursor)? It decides the first host in the M6-b matrix. (3) Which of your courses permit AI drafting, and which embedded tools appear in them? (4) Defaults to confirm: consumer selection, hidden-tab pause, event retention, desktop notifications. (5) Do you intend to distribute to other students? That triggers M8-d.

**C9. §1 — one sentence overstates.** "Existing Claude and OpenAI browser integrations already provide useful in-place assistance" is stated as fact; the dialogue verified their documentation only. Change to "document useful in-place assistance".

**C10. §3.3 step 4 — ordering.** "Metadata-only cached reads do not wait for model generation" reads as a latency claim. Change to "Metadata-only reads are served from the broker without a new account probe; the probe runs before any text is released."

**C11. §6 — two sources used in the turns are absent.** Add the MCP Rust SDK (`https://github.com/modelcontextprotocol/rust-sdk`, version `3.2.0`, supported revisions) because §3.2 relies on an adapter existing, and the Claude Code MCP page (`https://code.claude.com/docs/en/mcp`) for the scope and configuration statements in §3.2. Mark both as "documentation, not tested".

**C12. Header.** After applying corrections, change "Final peer review: pending" to "Final peer review: turn 07 applied" and add this file to the consensus line.

No other corrections. The design, tiers, plan and approval contract, attachment lifecycle, coordinator, event rules, packages, and evidence limits match turns 05 and 06.
