# ICS client behaviour (`canvas calendar --ics`)

`canvas calendar --ics PATH` writes one RFC 5545 file. `--ics -` streams the
same text to stdout. This page records how Apple Calendar and Google Calendar
treat that file on import and on re-import.

A `.ics` file is a **one-time export**. The client does not follow it, and it
does not refresh by itself. A change in Canvas becomes visible only after a
new export and a new import.

## Status of the observations

The five client checks below are **not yet recorded**. They need a Mac with
Apple Calendar and a Google account with Google Calendar, and they must be run
by a person who can see the result in each client. Fill in the tables when you
run them, and keep the wording to what you saw.

The file side of every case is covered by automated tests
(`crates/canvas-core/src/ics`, `crates/canvas-cli/tests/m4b.rs`), so the exact
lines named below are the lines the writer produces.

## Make the test files

```sh
# The whole default window.
canvas calendar --ics ~/canvas-window.ics

# The same window with a reminder one day before each deadline.
canvas calendar --ics ~/canvas-alarm.ics --alarm 24h

# Read the text without writing a file.
canvas calendar --ics - | less
```

Run the export twice with a change between the two runs to get the re-import
cases: change a due date in Canvas, or remove a calendar event, then export
again.

## The five cases

| Case | What the file carries |
|---|---|
| Changed due date | The same `UID` (`canvas-<kind>-<id>@<identity-key>`) with a new `DTSTART` and a new `DTSTAMP`. |
| Removed event | No `VEVENT` for that `UID`. The file has no `METHOD:CANCEL` and no `STATUS:CANCELLED`. |
| All-day event, profile zone west of UTC | `DTSTART;VALUE=DATE:<all_day_date>` and **no** `DTEND`. The civil date comes from Canvas and is never shifted through a timestamp. |
| All-day event across a DST change | The same one date. Canvas reports a 23-hour span; the command warns `all-day event with a longer span is shown as one day in v1` and still writes one day. |
| Canvas event with equal start and end | One `VEVENT` with `DTSTART;VALUE=DATE:<all_day_date>` and no `DTEND`. There is no warning: an equal start and end is Canvas' normal one-day form. |

A timed event keeps `DTSTART` and `DTEND` in UTC. A point deadline keeps
`DTSTART` only: no `DTEND` and no `DURATION`. `--alarm 24h` adds one `VALARM`
with `TRIGGER:-PT24H` to each deadline.

## Apple Calendar

Import: `File → Import…`, pick the file, then pick the target calendar.
Re-import: export again and import the new file into the **same** calendar.

| Case | Observed behaviour | Date observed |
|---|---|---|
| Changed due date |  |  |
| Removed event |  |  |
| All-day event west of UTC |  |  |
| All-day event across DST |  |  |
| Equal start and end |  |  |

Record for each case: whether the event was replaced or duplicated, the day
the event landed on, whether the event is shown as all-day, and whether the
alarm survived.

## Google Calendar

Import: `Settings → Import & export → Import`, pick the file, then pick the
target calendar.
Re-import: export again and import the new file into the **same** calendar.

| Case | Observed behaviour | Date observed |
|---|---|---|
| Changed due date |  |  |
| Removed event |  |  |
| All-day event west of UTC |  |  |
| All-day event across DST |  |  |
| Equal start and end |  |  |

Record the same four points as for Apple Calendar. Note also what the import
report says: Google Calendar reports how many events it added and how many it
did not add.

## Known limits

- A removed event stays in the client. The file cannot cancel it, so the
  student removes it by hand, or imports into a fresh calendar.
- `UID` carries the identity key, so two identities never overwrite each
  other's events in one calendar.
- The command writes UTC times. The client shows them in its own zone.
