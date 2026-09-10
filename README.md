# canvas-cli

`canvas-cli` is a Rust command-line client for Canvas LMS (Instructure), built for **students only**. One binary, `canvas`, gives a student their deadlines, Canvas-reported grades, course files, announcements, and a way to submit work with a durable journal and a local receipt, without opening the Canvas web app.

It is fast because it keeps a local SQLite cache. It is safe because the API token lives in the OS credential store and is sent only to its own origin. It is scriptable because every data command has a defined `--json` schema.

The first user is the owner, a student at Lasell University (`lasell.instructure.com`). The owner confirmed on 2026-09-09 that a personal access token works on that instance. The design works for any Canvas instance.
