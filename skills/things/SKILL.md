---
name: things
description: Operate Things 3 through the local `things` CLI or MCP server with verified writes, safe id resolution, and explicit handling of Things limitations.
---

# Things 3

Use this skill when the user asks to inspect, search, create, edit, complete,
reopen, delete, tag, organize into areas, empty the Trash, or import Things 3
items.

## Preconditions

1. Run `things doctor --json`.
2. Require `database_readable: true`.
3. For writes, require `things_app_installed: true` and `token_configured: true`
   when the operation uses `things:///update`.
4. If permission is denied, ask the user to grant Full Disk Access; do not try
   to bypass macOS permissions.

## Operating rules

- Prefer `things mcp` when the client supports MCP; otherwise use the CLI.
- Use `--json` for every machine-read call.
- Start with a small `--limit`.
- Listings carry at most 200 characters of notes and set `notes_truncated`;
  pass `--notes` for full text in a list, or use `things show <id>` for one item.
- Resolve by stable id returned from `list`, `search`, or `get`.
- Never guess when an operation returns `ambiguous`; choose a returned id or ask.
- Use `--dry-run` before a complex or bulk write.
- Read `verification.verified` after every write.
- If verification is false, read the item again before retrying.
- Never write the Things SQLite database directly.

## Recommended workflow

```sh
things doctor --json
things today --json --limit 20
things search "term" --json --limit 10
things show <id> --json
things edit <id> --deadline 2026-10-24 --dry-run --json
things edit <id> --deadline 2026-10-24 --json
things edit <id> --reminder 09:30 --dry-run --json
things edit <id> --reminder 09:30 --json
things area --json
things trash --json --limit 20
```

## Limitations to state explicitly

- Reminders can be set or replaced through `--reminder HH:MM`; creates default
  to today, while edits preserve the existing scheduled date.
- Reminder times must be strict 24-hour `HH:MM`. `anytime`, `someday`, and
  unscheduled items cannot carry a reminder.
- Clearing only a reminder is unsupported; use the Things app so the schedule
  is not changed accidentally.
- Repeating items reject `when`, `reminder`, `deadline`, and status changes.
- Repeating templates stay out of ordinary lists, including bare
  `--not-trashed` listings; use `things repeating` and address templates by
  full id. When an instance and template share a title, title lookups choose
  the instance.
- Recurrence rules are edited in Things; use `things repeat <id>`.
- The URL scheme cannot move an existing item to Inbox.
- Delete moves to Things Trash and requires confirmation; `empty-trash` is
  permanent and also requires `--yes`.
- An item that is already in the Trash cannot be deleted again; only
  `empty-trash` removes it.
- Areas can be created, renamed, and deleted; deleting an area keeps its
  projects and to-dos.
- Headings are read-only: Things has no scripting command for them.
- Items in Things Trash are reachable by id but are never matched by title.
- Unknown tags are ignored unless `--create-tags` is set.

## Bulk import

Validate the JSON array first:

```sh
things import payload.json --dry-run --json
things import payload.json --json
```

Read both returned arrays. Each top-level or nested to-do/project create is in
`created` with a `path`, `type`, `title`, `confirmed`, and, when confirmed, an
`id`. Each `operation: update` is in `updated` with its `id`, requested and
observed status, and a reason such as `target-not-found`,
`status-not-applied`, `fields-not-verified`, or `not-verified`. Same-title
creates are paired in creation order. A mixed update can report
`fields-not-verified` even after its status landed; only status-only updates
can be fully confirmed. Checklist-item updates accept only completed/canceled
and are read back from `TMChecklistItem`. Update destinations are preflighted
as references, so a valid move to another project/area is allowed. To-dos use
`list`/`list-id` and `heading`/`heading-id`; projects use `area`/`area-id`.
Ids must exist and match the item type; empty ids and fields for the wrong item
type are `conflict` errors. Nested updates must belong to their parent. If an item is
`not-found` or `ambiguous`, or the import used `--no-verify`, search for the
title before retrying; never claim full field verification. Headings are not
part of `created`.
