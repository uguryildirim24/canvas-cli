# Download course files

The user asks for a course's files, or for the readings in a module.

## Steps

1. `files.list` with the course to see what exists. `tree: true` groups by
   folder; `search` filters by name. `modules.list` with `items: true` shows
   which files belong to which module.
2. `download.plan` with the same filters. It reports, per file, what a
   download would do: transfer it, skip it because it is already there, or
   refuse it. It writes nothing.
3. Show the plan: the file count, the total size, and anything refused.
4. `download.run` with the same arguments once the user agrees.

## What the tool will not do

- It writes only into the configured destination. You cannot choose a path,
  and there is no `dest` argument on the agent surface. `canvas config get
  download.dir` shows where it goes.
- It never overwrites a file it does not own. There is no `force`.
- A file whose remote copy changed is written as a new revision, and the old
  one is kept.

## Partial results

Exit 12 is normal here: a locked file, an unavailable one, or one whose path
is unsafe. `result` carries a row per file and `partial` names the scopes that
failed. Report which files arrived and which did not, by name. Never say "the
download failed" when most of it worked.

`verify: true` re-reads the bytes of files that are already present and
reports a mismatch as exit 10. Use it when the user doubts a local copy.

## Typical calls

```
files.list    { "course": "CHEM", "tree": true }
modules.list  { "course": "CHEM", "items": true }
download.plan { "course": "CHEM" }
download.plan { "course": "CHEM", "module": "Week 3" }
download.run  { "course": "CHEM", "module": "Week 3" }
download.run  { "course": "CHEM", "files": [901, 902] }
download.run  { "course": "CHEM", "verify": true }
```
