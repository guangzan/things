# Architecture

中文版本：[ARCHITECTURE.zh-CN.md](ARCHITECTURE.zh-CN.md)

## Crates

`things-core` contains the domain model, read-only database access, URL-scheme
builders, `AppleScript` automation, verification, and the shared application
service.

`things` (in `crates/things-cli`) is the `clap` command line frontend. It maps
CLI arguments into core queries and write requests and renders text, JSON, or
NDJSON.

`things-mcp` exposes the same service over MCP stdio. Each request opens its own
read-only SQLite connection, avoiding cross-thread sharing of
`rusqlite::Connection`.

## Safety boundary

The live Things database is treated as a read-only source of truth:

1. Every connection uses `SQLITE_OPEN_READ_ONLY`.
2. Mutations are sent through `things:///` URLs or `osascript`.
3. Destructive operations require explicit confirmation (`--yes` or
   `options.yes: true`).
4. Direct writes to SQLite, including recurrence internals, are unsupported.

The URL and `AppleScript` paths are the supported public automation interfaces.
They may still be asynchronous: Things can accept a request before committing it
to SQLite. Verification therefore polls with a bounded timeout.

## Write verification

The verifier does not assume that a sent URL succeeded.

- Creates snapshot all existing task/project ids first and only accept a new id.
- Edits compare the item before and after the write against the requested
  fields.
- Status changes poll for the exact target status.
- Deletes poll until the item is marked trashed.
- Tag operations poll for presence, absence, or the new title.

`verification.verified = false` means the request was sent but the expected
database state was not observed within the timeout. It is not a claim that the
write failed.

## Query model

`ItemQuery` is shared by CLI and MCP. Views apply the same base predicates as the
Things sidebar; filters are then composed with SQL `AND`.

List and search pages carry only a 200-character, UTF-8-safe note preview and
set `notes_truncated` when text was cut. `get` bypasses the preview and returns
the complete note.

`ItemQuery::sort` defaults to `index`, which means "the view's natural order".
For `today`, `upcoming`, and `deadlines` the stored `TMTask."index"` is a
per-list position and is not meaningful, so the query substitutes the app's own
ordering (see [COMMANDS.md](COMMANDS.md)). The merged todo + project listing in
`Service::list` re-sorts in memory with the same key, so a view never mixes two
orderings.

Regex filters are evaluated in Rust because SQLite does not provide a regexp
function by default. The resulting UUID set is injected as an `IN` predicate
with escaped identifiers.

Resolution order:

1. Exact id.
2. Exact title (case-insensitive) across todos, projects, and headings.
3. Title substring across todos, projects, and headings.
4. Area or tag title substring, or area/tag id prefix.

If more than one candidate remains, the call fails with `ambiguous` and returns
stable ids.

## Things data encoding

- Status: `0 = open`, `2 = canceled`, `3 = completed`
- Type: `0 = todo`, `1 = project`, `2 = heading`
- Scheduled bucket (`start`): `0 = inbox`, `1 = anytime`, `2 = someday`
- `startBucket` is a separate column: `1` marks This Evening, not a bucket
- Today position: `todayIndex` with `todayIndexReferenceDate`
- Packed date: `year << 16 | month << 12 | day << 7`
- Reminder time: `hour << 26 | minute << 20`
- Timestamps: Unix epoch seconds

The URL auth token is read from `TMSettings` when it is not supplied in config or
the environment.

## Error model

Core errors have stable categories such as `not_found`, `ambiguous`,
`invalid_input`, `conflict`, `permission_denied`, `auth_required`,
`app_unavailable`, and `verification_failed`. The CLI maps these to JSON error
objects and process exit codes.

MCP distinguishes transport from tool outcomes. Unknown tools and schema
decoding failures are protocol errors. Core business failures return a
successful tool response with `isError: true` and structured error data, which
lets agents recover by inspecting candidates or changing arguments without
treating the MCP session as broken. Read-only mode removes write tools from
`tools/list` and also rejects direct calls to them.
