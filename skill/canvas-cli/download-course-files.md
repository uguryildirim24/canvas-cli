# Download course files

The user asks for a course's files, or for the readings in a module.

## Steps

1. `files.list` with the course to see what exists. `tree: true` groups by
   folder; `search` filters by name. `modules.list` with `items: true` shows
   which files belong to which module.
2. A download is a `canvas` command, not a tool: the MCP catalog is reads
   only. `canvas download <course> --dry-run --json` reports, per file, what
   a download would do — transfer it, skip it because it is already there, or
   refuse it — and writes nothing.
3. Show the plan: the file count, the total size, and anything refused.
4. `canvas download <course>` with the same filters once the user agrees.

## What the command will not do

- It writes only into the configured destination unless the person names
  another one themselves. `canvas config get download.dir` shows where it
  goes. Never choose a path on the user's behalf.
- It never overwrites a file it does not own. There is no `force`.
- A file whose remote copy changed is written as a new revision, and the old
  one is kept.

## Partial results

Exit 12 is normal here: a locked file, an unavailable one, or one whose path
is unsafe. `result` carries a row per file and `partial` names the scopes that
failed. Report which files arrived and which did not, by name. Never say "the
download failed" when most of it worked.

`--verify` re-reads the bytes of files that are already present and reports a
mismatch as exit 10. Use it when the user doubts a local copy.

Never pass `--force`. It overwrites a file the tool does not own, and that is
the person's decision, not yours.

## Typical calls

The reads are tools. The transfer is a command.

```
files.list   { "course": "CHEM", "tree": true }
modules.list { "course": "CHEM", "items": true }
```

```sh
canvas download CHEM --dry-run --json
canvas download CHEM --module "Week 3" --dry-run --json
canvas download CHEM --module "Week 3"
canvas download CHEM --file 901 --file 902
canvas download CHEM --verify
```
