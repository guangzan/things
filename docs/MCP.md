# MCP server

中文版本：[MCP.zh-CN.md](MCP.zh-CN.md)

Run the server over stdio:

```sh
things mcp
```

Example Codex configuration:

```toml
[mcp_servers.things]
command = "/absolute/path/to/things"
args = ["mcp"]
```

Example generic JSON configuration:

```json
{
  "mcpServers": {
    "things": {
      "command": "/absolute/path/to/things",
      "args": ["mcp"]
    }
  }
}
```

For a read-only agent:

```json
{
  "mcpServers": {
    "things": {
      "command": "/absolute/path/to/things",
      "args": ["mcp", "--read-only"]
    }
  }
}
```

## Tools

### Read tools

| Tool | Purpose |
|---|---|
| `list` | List any view with boolean/regex filters |
| `list_todos` | List todos only |
| `list_projects` | List projects only |
| `search` | Search titles and notes with list-style filters |
| `get` | Resolve one item and return full detail |
| `list_areas` | Areas with tags and counts |
| `list_tags` | Tags with usage counts |
| `list_headings` | Headings in a project |
| `stats` | Schema, counts, and database metadata |
| `get_agent_brief` | AI-oriented operating instructions |

### List input

`list`, `list_todos`, and `list_projects` share one input object. All filters
are optional and combine with AND:

```json
{
  "view": "logbook",
  "project": "Q4 Planning",
  "tags_all": ["focus"],
  "text": "report",
  "created_from": "2026-01-01",
  "modified_to": "2026-10-19",
  "completed_after": "2026-10-01",
  "completed_before": "2026-10-19",
  "sort": "stop_date",
  "desc": true,
  "limit": 50
}
```

Date bounds use inclusive `YYYY-MM-DD` values. `completed_after` and
`completed_before` match the completion or cancellation timestamp, so they are
most useful with `view: "logbook"`. `heading` limits results to one heading
inside the selected project. Calls without a `view` and without `trashed` cover
active items only; set `"trashed": true` or use the `trash` view to include
Things Trash.

Repeating templates and the tasks inside repeating project templates are
excluded from ordinary active listings, even when `"trashed": false` is set
explicitly. Use `"view": "repeating"` to list templates. An explicit
`"trashed": true` listing and the logbook report stored rows literally,
templates included. When a generated instance and its template share a title,
`get` and all title-based write tools resolve to the generated instance; use
the template's UUID to target it directly.

`types` accepts `"todo"`, `"project"`, and `"heading"`. Headings are never part
of an implicit listing; they appear only when `"heading"` is requested
explicitly.

`list` and `search` return at most 200 characters of notes and set
`notes_truncated: true` when the text was shortened. `get` always returns the
full note. Set `include_notes: true` to request full notes in a listing.

`search` accepts `query` plus the same scoping filters as `list`:
`view`, `status`, `types`, `project`, `area`, `heading`, `tag`, `tags_all`,
`tags_any`, `tags_none`, `include_closed_project_tasks`, `sort`, `desc`,
`limit`, `offset`, `include_notes`, and `include_checklist`. This narrows a text
search without fetching every match first. `query` is required and must contain
non-whitespace text.

Active listings hide open to-dos left behind in completed, canceled, or
trashed projects, matching the Things sidebar. Set
`"include_closed_project_tasks": true` to include that archived context.

References (`target`, `project`, `area`, `heading`, and tag names) resolve by
full id, id prefix of four or more characters, exact title, or unique title
substring. Items in Things Trash are reachable by id but are never matched by
title, so a fuzzy reference cannot silently hit something the user deleted.
Container references are type-scoped: `project` only matches projects,
`area` only matches areas, and tag filters only match tags. For tag filters,
the resolved canonical tag title is sent to `SQLite`, so an id prefix and a
tag name behave the same way.

### Write tools

These are hidden when `read_only = true` or `things mcp --read-only` is used.

| Tool | Purpose |
|---|---|
| `add_todo` | Create and verify a to-do |
| `add_project` | Create and verify a project |
| `edit` | Partially edit a todo or project |
| `set_status` | Open, complete, or cancel |
| `delete` | Move to Things Trash; `options.yes` required |
| `empty_trash` | Permanently empty Things Trash; `options.yes` required |
| `add_tag` | Create a tag |
| `rename_tag` | Rename a tag |
| `delete_tag` | Delete a tag; `options.yes` required |
| `add_area` | Create an area |
| `rename_area` | Rename an area |
| `delete_area` | Delete an area; `options.yes` required |
| `import` | Send a Things JSON array and read back created items |
| `open` | Reveal an item or view |
| `open_repeat_editor` | Open the official repeat editor |

`edit` accepts checklist replacement and append fields plus the three
checklist status arrays:

```json
{
  "target": "<todo id>",
  "complete_checklist": ["Collect numbers"],
  "cancel_checklist": ["Obsolete step"],
  "reopen_checklist": ["Retry import"]
}
```

Checklist items are addressed by exact title. Each write tool accepts
`options` as described below.

`add_todo`, `add_project`, and `edit` accept `reminder` as a strict 24-hour
`HH:MM` string. Reminders are encoded in `when` as `date@HH:MM`: creates use
the supplied date or default to today, and edits preserve the item's existing
scheduled date. `anytime`, `someday`, and unscheduled items return `conflict`.
Setting a reminder replaces an existing one; clearing only a reminder returns
`unsupported`. `edit` therefore exposes `clear_reminder` so agents get a clear,
typed failure rather than silently dropping the request.

### Import input

`import` takes the Things JSON URL-scheme payload as a string in `data`:

```json
{
  "data": "[{\"type\":\"to-do\",\"attributes\":{\"title\":\"Buy milk\"}}]",
  "options": {"verify": true}
}
```

With verification enabled, the result includes a `created` array for creates
and an `updated` array for `operation: update` items, both in payload order.
Each created entry identifies the payload `path`, `type`, `title`, `confirmed`,
and, when confirmed, the database `id`:

```json
{
  "action": "import",
  "sent": true,
  "verification": {"verified": true},
  "created": [
    {
      "path": "[0].attributes.items[0]",
      "type": "todo",
      "title": "Buy milk",
      "confirmed": true,
      "id": "..."
    }
  ]
}
```

Updated entries carry `path`, `type`, `id`, `title`, `confirmed`,
`wanted_status`, and `observed_status`. A missing target reports
`target-not-found`; a requested status that did not land reports
`status-not-applied`; a status change that landed alongside unverified fields
reports `fields-not-verified`; an update without a status request reports
`not-verified`. Skipping verification returns `no-verify`, and dry-run returns
`dry-run`.

Create verification proves identity, not complete field verification: it
confirms that a new row of the same type and exact title appeared after the
pre-write snapshot. When a payload creates several items with the same type and
title, they are paired in creation order. Extra rows or identical creation
timestamps return `ambiguous` with candidates; fewer rows return `not-found`.
Update verification covers a status-only request. Mixed-field updates remain
unconfirmed even when the status landed, and the top-level
`verification.verified` is false. Checklist-item updates may only change
completed/canceled and are read back from `TMChecklistItem`. Do not retry an
unconfirmed item blindly. Headings and checklist items are not included in
`created`.

Update destinations are checked before sending. To-dos accept `list`/`list-id`
(a project or area) and `heading`/`heading-id`; projects accept
`area`/`area-id`. An id must name an existing Things id of the compatible type.
The check validates the destination, not the item's current parent, so a
legitimate move is accepted. Empty ids, fields that do not apply to the item
type, and ambiguous or missing destinations return `conflict`. Nested
todo/checklist updates must belong to their containing project/to-do.

## Write options

Write tools accept an `options` object:

```json
{
  "dry_run": false,
  "verify": true,
  "foreground": false,
  "create_missing_tags": false,
  "strict_tags": false,
  "yes": false
}
```

## Result shape

Successful writes return:

```json
{
  "action": "edit_todo",
  "type": "todo",
  "id": "...",
  "title": "...",
  "sent": true,
  "url": "things:///update?...",
  "verification": {
    "verified": true,
    "attempts": 3,
    "elapsed_ms": 742,
    "observed": "..."
  },
  "item": {}
}
```

Treat `verification.verified = false` as **sent but unconfirmed**, not as a
definite failure. Read the item again before retrying a non-idempotent write.

## Error semantics

Protocol errors are reserved for malformed JSON-RPC, unknown or disabled tools,
and arguments that do not match a tool's input schema. The latter is returned as
a JSON-RPC error with code `-32602`; it is not a tool result.

Business failures are successful MCP tool calls with `isError: true` and
structured content:

```json
{
  "code": "ambiguous",
  "message": "multiple items match \"o\"",
  "candidates_truncated": false,
  "candidates": [
    {
      "id": "...",
      "type": "todo",
      "title": "Write report"
    }
  ]
}
```

At most 25 candidates are returned. When `candidates_truncated` is `true`, more
matches existed than the response carries; narrow the reference or list with a
filter instead of treating the returned ids as the complete set.

This includes `not_found`, `ambiguous`, `invalid_input`, `invalid_date`,
`conflict`, `unsupported`, `permission_denied`, `auth_required`,
`app_unavailable`, `verification_failed`, and internal or database failures.
Clients should inspect `isError` and `structuredContent.code` rather than
assuming the transport failed.

Tool annotations mark every read tool with `readOnlyHint: true`. `delete`,
`empty_trash`, `delete_tag`, and `delete_area` also set
`destructiveHint: true`; callers should request explicit confirmation. Those
tools fail with `conflict` unless `options.yes` is `true`.

## Things limitations

- Reminders can be set or replaced. They require a concrete date, `today`,
  `tomorrow`, or `evening`; `anytime` and `someday` are rejected.
- Creating without `when` defaults a reminder to today. Editing preserves the
  item's existing scheduled date and rejects unscheduled items.
- Clearing only a reminder is unsupported because the URL scheme cannot express
  it without changing the schedule. Use the Things app.
- Repeating items reject `when`, `reminder`, `deadline`, and status changes.
- The URL scheme cannot move a to-do to Inbox.
- Recurrence rules are edited in the Things UI.
- Unknown tags are ignored unless `create_missing_tags` is true.
- Headings cannot be created, renamed, or deleted; Things exposes no scripting
  command for them.
- `empty_trash` cannot be undone.
- Removing a reminder without changing the schedule, and other attributes
  without a scripting command, require the Things UI.

See [COMPATIBILITY.md](COMPATIBILITY.md) for details.
