# Command reference

中文版本：[COMMANDS.zh-CN.md](COMMANDS.zh-CN.md)

Global options can appear before or after a subcommand:

```text
--db <PATH>          Explicit Things main.sqlite
--read-only          Disable all writes
--json               JSON output
--format <FORMAT>    text, json, or ndjson
--dry-run            Build a write without sending it
--no-verify          Skip post-write verification
--foreground         Bring Things forward during a write
-y, --yes            Confirm destructive or project-wide operations
--create-tags        Create unknown tags before using them
--strict-tags        Reject unknown tags
```

## Views

```sh
things today [FILTERS]
things inbox [FILTERS]
things upcoming [FILTERS]
things anytime [FILTERS]
things someday [FILTERS]
things logbook [FILTERS]
things trash [FILTERS]
things deadlines [FILTERS]
things repeating [FILTERS]
things projects [FILTERS]
things list --view today [FILTERS]
```

## Filters

```text
--status open,completed,canceled
--type todo,project,heading
--project <REF>              Project id, prefix, title, or substring
--area <REF>                 Area id, prefix, title, or substring
--heading <REF>              Heading id or title
--tag <NAME>                 Require one tag
--tags-all a,b               Require every tag
--tags-any a,b               Require at least one tag
--tags-none a,b              Exclude tags
--text <TEXT>                Title/notes substring
--title-regex <REGEX>        Rust regex
--notes-regex <REGEX>        Rust regex
--created-from/--created-to  YYYY-MM-DD
--modified-from/--modified-to
--completed-after/--completed-before   Completion/cancellation date (Logbook)
--start-from/--start-to
--deadline-from/--deadline-to
--overdue
--has-deadline / --no-deadline
--has-reminder / --no-reminder
--has-tags / --no-tags
--has-checklist / --no-checklist
--trashed / --not-trashed
--closed-projects            Include open to-dos left in closed or trashed projects
--repeating / --not-repeating
--sort index|title|created|modified|start_date|deadline|stop_date
--desc
--limit N --offset N
--notes                      Include full notes (listings default to a 200-character preview)
--checklist                  Include checklist items
```

Listings without a `--view` and without an explicit `--trashed` /
`--not-trashed` cover active items only; pass `--trashed` (or use the `trash`
view) to include items in Things Trash.

Repeating templates and the tasks stored inside repeating project templates
belong to `things repeating`, not to ordinary active listings. This holds with
or without an explicit `--not-trashed`: `things list --status open
--not-trashed` still omits them. An explicit `--trashed` listing and the
`logbook` view report stored rows literally, so templates there are included.

When a generated task and its repeating template share a title, title-based
lookups (`show`, `edit`, `done`, `cancel`, and `reopen`) prefer the generated
task. Use an id to address the template directly; a template-only title still
resolves.

Active views hide open to-dos that were left behind when their project was
completed, canceled, or trashed, matching Things. Pass `--closed-projects` to
include that archived context, for example when auditing a deleted project.
The `trash` view likewise lists the trashed projects and standalone to-dos the
app shows, not every child row stored under a trashed project.

`--type heading` returns headings across every project. Headings never appear in
an implicit listing; ask for them by type.

List, search, and view output carries at most 200 characters of each note and
sets `notes_truncated: true` when text was cut. Pass `--notes` to get full notes
in a listing; `things show` always returns the complete note.

### Default ordering

`--sort index` is the default and means "the order Things itself shows for the
selected view", not a raw database column:

| View | Natural order |
|---|---|
| `today` | regular items first, then This Evening; within a group newest `todayIndexReferenceDate`, then `todayIndex` |
| `upcoming` | soonest start date, then soonest deadline; undated items last |
| `deadlines` | soonest deadline first |
| `logbook` | most recently completed or canceled first (`stopDate` descending) |
| other views | the stored list position |

Every order ends with a stable id tiebreak, so paging with `--limit`/`--offset`
never reorders equal keys. `--desc` returns the reverse of the selected order.

## Read and search

```sh
things search "query" [FILTERS]
things show <ID|PREFIX|TITLE>
things area
things area list
things headings <PROJECT>
things stats
things agent-brief
things doctor
```

Ambiguous references return an error with candidates. Use a longer id prefix or
the full id. At most 25 candidates are returned; `candidates_truncated: true`
means more matches existed, so narrow the reference rather than assuming the
list is complete. References resolve by full id, id prefix of four or more
characters, exact title, or unique title substring; items in Things Trash stay
reachable by id but are never matched by title.

Filter references are type-scoped: `--project` only matches projects, `--area`
only matches areas, and tag filters only match tags. This matters when an area
and a tag share a name.

## Create and edit

```sh
things add "Title" \
  --notes "Details" \
  --when tomorrow \
  --reminder 09:30 \
  --deadline 2026-10-24 \
  --tags work,focus \
  --checklist "First step" \
  --project <PROJECT> \
  --heading <HEADING>

things edit <TARGET> \
  --title "New title" \
  --append-notes "More detail" \
  --when tomorrow \
  --reminder 09:30 \
  --deadline 2026-10-24 \
  --add-tags urgent \
  --append-checklist "Another step" \
  --complete-checklist "First step"
```

Use `--clear-when` or `--clear-deadline` to clear those fields. Use
`--status completed` or `--status canceled` only when appropriate; `done`,
`cancel`, and `reopen` are clearer for status-only changes.

`--reminder HH:MM` sets or replaces a reminder for a to-do or project. The
value must be a strict 24-hour time; `9:00` and `24:00` are rejected. It is
encoded in the Things URL as `when=date@HH:MM`. With `--when`, the reminder
uses that date. Without `--when` on create, it defaults to today. On edit it
keeps the item's current scheduled date, so setting a reminder does not move
the item to today. `anytime`, `someday`, and unscheduled items cannot carry a
reminder. Clearing only a reminder is unsupported by the URL scheme; use the
Things app instead. Repeating items reject reminder changes.

Checklist items are addressed by exact title:

```text
--complete-checklist <TITLE>   Mark a checklist item completed
--cancel-checklist <TITLE>     Mark a checklist item canceled
--reopen-checklist <TITLE>     Reopen a completed or canceled checklist item
```

Each option may be repeated. Missing or ambiguous checklist titles fail before
the write is sent.

Checklist status changes are sent as a separate Things JSON update so their
result can be read back from `TMChecklistItem`. They cannot be combined with
other edits in one command.

## Projects

```sh
things project add "Launch" \
  --area Work \
  --todo "Draft plan" \
  --todo "Review plan"

things project edit <PROJECT> --append-notes "Kickoff notes"
```

Closing or reopening a project requires `--yes`, because it affects every task
inside the project.

## Status and deletion

```sh
things done <TARGET>
things cancel <TARGET>
things reopen <TARGET>
things delete <TARGET> --yes
things empty-trash --yes
```

`delete` moves the item to Things Trash. It does not permanently erase the
database row. `empty-trash` permanently deletes everything in the Trash and
cannot be undone, so it always requires `--yes`.

Deleting an item that is already in the Trash returns a `conflict` error.
Things cannot address trashed items individually, so `empty-trash` is the only
way to remove them.

Repeating items reject `when`, `reminder`, `deadline`, and status changes; use
`things repeat <ITEM>` to edit the rule in Things.

## Tags

```sh
things tag list
things tag add "deep-work"
things tag rename <TAG> "new-name"
things tag delete <TAG> --yes
```

Tag names cannot contain commas, which Things uses as a separator.

## Areas

```sh
things area
things area add "Work"
things area rename <AREA> "Deep work"
things area delete <AREA> --yes
```

`things areas` is accepted as an alias for `things area list`. `area add` returns
an existing area when the name already matches, so it never creates a duplicate.
Deleting an area moves it to Things Trash; the projects and to-dos inside it are
not deleted.

## Import

```sh
things import payload.json
cat payload.json | things import -
things import payload.json --dry-run --json
```

The payload must be a JSON array in the format accepted by
`things:///json`.

With verification enabled (the default), `things import --json` returns a
`created` array for top-level and nested to-do/project creates and an `updated`
array for `operation: update` items, both in payload order. Created entries have
`path`, `type`, `title`, `confirmed`, and, when found, `id`. Updated entries
also have `id` plus `wanted_status` and `observed_status` when the update asks
for a status change.

Created entries can report `not-found`, `ambiguous`, `no-verify`, or `dry-run`.
Updated entries can additionally report `target-not-found`,
`status-not-applied`, `fields-not-verified`, or `not-verified`. For creates,
`confirmed` verifies identity only: it proves that a new row with the expected
type and title appeared after the pre-write snapshot, not that every attribute
was applied.

For updates, `confirmed` covers a status-only update. If an update combines
`completed` or `canceled` with another field, the status may land but the entry
still reports `fields-not-verified`; the top-level `verification.verified` is
then false. An update without a status request reports `not-verified`.
Checklist-item updates support status only and are verified from
`TMChecklistItem`; other checklist attributes are rejected as `unsupported`.
Headings and checklist items are not part of `created`.

Update destinations are preflighted independently of the item's current
placement, so a valid move does not fail merely because it points elsewhere.
To-dos accept `list`/`list-id` (a project or area) and `heading`/`heading-id`;
projects accept `area`/`area-id`. An id must name an existing Things id of the
compatible type. Empty ids, fields that do not apply to the item type, and
unknown or ambiguous destinations return `conflict` before the payload is sent.
An empty `list-id` is not treated as Inbox. Nested todo and checklist updates
must belong to their enclosing project or to-do.

When an item is unconfirmed, search for its title before retrying so a retry
does not duplicate it.

## Open and repeat

```sh
things open <TARGET>
things open --view today
things repeat <TARGET>
```

`repeat` opens the item in Things. Direct recurrence-rule writes are
intentionally unsupported.

## MCP and configuration

```sh
things mcp
things config path
things config show
```

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | Success |
| 1 | Database or internal failure |
| 2 | Invalid input, date, conflict, or unsupported operation |
| 3 | Permission, authentication, or Things app unavailable |
| 4 | Not found or ambiguous target |
| 5 | Write sent but verification failed |
