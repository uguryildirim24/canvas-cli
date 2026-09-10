# Agent-first Canvas: design dialogue brief

Two agents work on this: **fable** (Claude Fable 5.1, high effort) and **gpt**
(Codex, ChatGPT Pro). You share this worktree. You talk to each other only
through files under `docs/agent-ux/turns/`. A human (Rolf, a Lasell
University student) reads the final report; Claude (the coordinator) folds it
into the build plan after the current spec (`docs/SPEC.md` v0.8) is
implemented.

## The question

`canvas-cli` (read `docs/SPEC.md` §0–§5, §7, §12.2, §13, §18 first) is a
student-only Rust CLI for Canvas LMS with a JSON contract, a local cache, a
submission journal, and downloads. Rolf uses AI agents (Claude Code, Codex,
Cursor) to organize and help with his coursework. Canvas itself runs in a
separate browser tab: assignments, quizzes, discussions, grades, pages.

Design **how canvas-cli becomes a speedy and powerful tool for any agent**,
and **how the agent can "come with Rolf" into the Canvas browser tab when he
asks** (see what he sees, help in place, act where appropriate), so that the
CLI plus the agent become his **single interaction with Canvas**.

Rolf's first idea is browser automation ("browser use"). Treat it as one
candidate, not the answer. Consider at least: an MCP server mode of the CLI
(`canvas mcp`) and other agent-native surfaces; a browser extension or side
panel that bridges the open Canvas tab to the agent (compare with existing
"Claude in Chrome" / Codex browser integrations and what they can already
do); connecting to the user's existing Chrome via CDP versus driving a
separate browser; deep links (`canvas open`) plus a companion that follows;
reading the DOM of the current tab versus using the API for the same
content; session/cookie reuse and why SPEC §1 excludes cookie import in v1;
watch/notify/event streams for agents; what Canvas offers that the spec does
not yet use (GraphQL, ICS feeds, notifications, inbox, discussions, LTI,
the Student mobile app deep links, Canvas Studio, Lasell-specific setup);
latency budgets for an agent loop (what must be under 100 ms, what can take
seconds); auth and safety boundaries (token scope, what the agent may do
without asking, what always needs Rolf's confirmation).

Keep SPEC §1 non-goals for **v1** as given (no quiz taking, no teacher
tools, no group submissions). For **v2+** you may propose changing any of
them, but say explicitly what changes and why, including academic-integrity
implications of an agent present during quizzes and graded work.

Research is expected: use web search and the Canvas API docs and source
(`https://github.com/instructure/canvas-lms`), the Chrome extension/CDP
docs, MCP specs, and existing tools (`docs/research/r2-prior-art.md` lists
prior art). Cite what you rely on.

## Protocol

- Turn files: `docs/agent-ux/turns/NN-<fable|gpt>.md`, NN zero-padded.
  **fable writes odd turns (01, 03, …), gpt writes even turns (02, 04, …).**
- A turn is 300–1200 words: respond to the other's points by number, add
  new proposals, mark what you now agree with, and end with either
  `NEXT: <other>` or `AGREED` (you believe the design is settled).
- After writing your turn, **wait for the other's file** with a shell loop,
  e.g. `until [ -f docs/agent-ux/turns/04-gpt.md ]; do sleep 20; done`.
  If your tool limits a single command to a few minutes, run the loop in
  slices (8 minutes each) until the file appears. Give up after 60 minutes
  of silence: write `docs/agent-ux/STALLED.md` saying who you waited for.
- When two consecutive turns both say `AGREED`, or after turn 12, the
  author of the **last** turn writes `docs/agent-ux/REPORT.md` and the
  other reviews it in one final turn file named `NN-<who>-review.md`
  (corrections only). Then the report author applies the corrections and
  prints exactly: `DONE docs/agent-ux/REPORT.md`.
- Commit each of your own files on this branch (`spec/agent-ux`) right after
  writing it: `git add docs/agent-ux && git commit -m "agent-ux: turn NN"`.
  Pull nothing, push nothing. Do not edit the other agent's files or
  anything outside `docs/agent-ux/`.

## REPORT.md contents

1. One-paragraph answer for Rolf.
2. Options considered, with a comparison table (latency, safety, effort,
   what it enables, what breaks).
3. Recommended architecture with a diagram (Mermaid), the agent-facing
   surface (commands/tools and their contracts), the "come with me" flow
   step by step, and the confirmation/safety model.
4. What to add to canvas-cli, mapped as new §18 packages after M5, in
   dependency order, each with acceptance criteria.
5. Risks and open questions for Rolf.
6. Sources.
