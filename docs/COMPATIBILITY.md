# Compatibility and limitations

中文版本：[COMPATIBILITY.zh-CN.md](COMPATIBILITY.zh-CN.md)

## Tested environment

- macOS with Things 3 `3.24.1`
- Things database schema version `29`
- Rust 1.89+ (2024 edition)

Things does not publish a stable SQLite schema. The reader uses defensive column
defaults, but a future Things release can still change table or column names.
Run `things doctor` after upgrading Things.

On startup the reader verifies that every table and column it relies on exists.
A mismatch fails fast with an `unsupported` error naming the missing
table/column, instead of silently returning empty or partial results.

## View ordering

`today`, `upcoming`, and `deadlines` ignore the stored `"index"` column, which is
a per-list position and not meaningful for these date-scoped views. They follow
the app's own ordering instead (see the "Default ordering" table in
[COMMANDS.md](COMMANDS.md)). The This Evening section of Today is identified by
`startBucket = 1`, and Today also includes Someday items whose scheduled date has
already passed.

## Supported write paths

| Operation | Mechanism | Verified |
|---|---|---|
| Create to-do | `things:///add` | New id observed, including date/time reminder |
| Create project | `things:///add-project` | New id observed, including date/time reminder |
| Edit to-do | `things:///update` | Before/after fields, including date/time reminder |
| Edit project | `things:///update-project` | Before/after fields, including date/time reminder |
| Complete/cancel/reopen | `AppleScript` status | Exact status |
| Delete | `AppleScript` delete | Trashed flag |
| Empty Trash | `AppleScript` empty trash | Trash count reaches zero |
| Create tag | `AppleScript` | Tag appears |
| Rename tag | `AppleScript` | New title appears |
| Delete tag | `AppleScript` | Tag disappears |
| Create area | `AppleScript` | Area appears |
| Rename area | `AppleScript` | New title appears |
| Delete area | `AppleScript` | Area disappears |
| Bulk import | `things:///json` | Prevalidated; per-item identity read-back |

## Known limitations

### Reminders

The Things URL scheme has no separate `reminder` parameter, but it does accept
a time appended to `when` as `date@HH:MM`. The CLI and MCP accept a strict
24-hour `HH:MM` reminder and encode it that way, so setting or replacing a
reminder is supported for to-dos and projects.

Reminders require a concrete date, `today`, `tomorrow`, or `evening`.
`anytime` and `someday` have no date to attach a reminder to and return
`conflict`. On create, omitting `when` defaults to the current date. On edit,
the item's existing scheduled date is preserved; if the item has no concrete
date, the edit returns `conflict`. Setting a reminder on an item that already
has one replaces it.

Clearing only the reminder is unsupported: the URL scheme cannot express that
without also changing the schedule, so the CLI/MCP return `unsupported` and
direct the user to the Things app. Repeating items reject reminder changes
along with `when`, deadline, and status changes.

### Repeating items

Things does not permit changing `when`, `deadline`, or status on a repeating
item through the URL scheme. The tool returns `unsupported` and directs the user
to the repeat editor.

The CLI/MCP can read recurrence state from `rt1_recurrenceRule` and, when the
column exists, the legacy `repeater` blob. It does not write either
undocumented field.

### Inbox

The public URL scheme cannot move an existing to-do back to Inbox. The tool does
not attempt a database mutation to simulate it.

### Tags

Things uses commas to separate tags in URLs. Tag names containing commas are
rejected. Unknown tags are ignored by Things unless tag creation is enabled.

### Delete

Delete is an `AppleScript` operation and enters Things Trash. A single item is
never erased permanently; `things empty-trash --yes` is the only permanent
deletion, and it always needs explicit confirmation.

Things' `AppleScript` dictionary cannot address an item that already sits in
the Trash: `to do id "<id>"` fails with error `-1728`, and no command exposes
the Trash contents individually. Deleting an already-trashed item therefore
returns a `conflict` error that points at `things empty-trash --yes` instead
of attempting the operation.

### Headings

Things exposes no scripting command for headings, so they can be read and used as
a filter, but not created, renamed, or deleted. Headings and reminder removal
without changing an item's schedule are edited in the Things UI.

### Verification

Things can commit asynchronously. Verification is bounded by
`verify_timeout_ms`. A timeout reports `verified = false`; it does not prove the
write failed.

### Bulk import verification

Import payloads are validated before dispatch: the root must be a JSON array,
items must have a supported `type`, required string fields must be non-empty,
field types and duplicate JSON keys are checked, and each batch is limited to
200 create operations. The official JSON URL is then sent as one batch.

Things does not return per-item identifiers for JSON imports, so this tool
reads the database after the write. Every created to-do and project is matched
by type and exact title against rows that did not exist in the pre-write
snapshot. Nested to-dos are also matched to their parent project (by project id
for an updated project, or title for a newly created project). Same-title items
are paired in creation order; missing, extra, or timestamp-tied rows are
reported as `not-found` or `ambiguous` with candidates. `no-verify` returns
every expected item as unconfirmed.

`operation: update` items are returned in a separate `updated` array. Status
changes are polled to the exact requested status; a missing row reports
`target-not-found`, a status that did not land reports `status-not-applied`,
and an update with no status request reports `not-verified`. A mixed update
(for example `completed: true` plus `notes`) reports `fields-not-verified`
even when the status was observed, and the top-level verification stays false.
Only a status-only update can be fully confirmed. Repeating items are refused
before dispatch when the update touches `when`, `deadline`, `completed`, or
`canceled`.

Checklist-item updates are resolved through their parent to-do and may only
request `completed` or `canceled`; their status is read directly from
`TMChecklistItem`. Other checklist fields are rejected as `unsupported`.
Update destinations are reference-checked before dispatch. To-dos accept
`list`/`list-id` (a project or area) and `heading`/`heading-id`; projects accept
`area`/`area-id`. An id must be an existing Things id of the compatible type.
Empty ids and destination fields that do not apply to the item type are
`conflict` errors rather than Inbox moves or silent no-ops. Nested
todo/checklist updates must belong to their enclosing container. The
destination does not have to equal the item's current parent, because moving an
item is a supported update.

This is best-effort identity verification, not complete field verification. A
confirmed created entry proves that a new row with that type and title
appeared; it does not prove that every attribute in the payload was applied.
Headings and checklist items are not included. Treat `confirmed: false` as
requiring a search before retry, not as proof that the item is absent.

## Read-only mode

Read-only mode removes write tools from MCP and rejects write commands in the
CLI. It also prevents opening URLs that mutate data, while still allowing reads.

## Privacy

The tool reads the local Things database and never uploads data. MCP stdio
traffic goes to the connected client, so review that client's own privacy
settings.
