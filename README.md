<div align="center">

# things

中文版本：[README.zh-CN.md](README.zh-CN.md)

**AI-first Things 3 CLI and MCP server for macOS**

[![CI](https://img.shields.io/github/actions/workflow/status/guangzan/things/ci.yml?style=flat-square&label=CI)](https://github.com/guangzan/things/actions)
[![Rust](https://img.shields.io/badge/Rust-1.89+-dea584?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-macOS-black?style=flat-square&logo=apple&logoColor=white)](https://culturedcode.com/things/)
[![License](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)

[Features](#features) · [Install](#install) · [Quick start](#quick-start) · [MCP](#mcp) · [Configuration](#configuration) · [Docs](#documentation)

</div>

`things` is a production-oriented command line interface and MCP server for
[Things 3](https://culturedcode.com/things/). It is written in Rust and built
around a strict safety boundary:

- **Reads** query the live Things SQLite database opened read-only
- **Writes** go through the official Things URL scheme or AppleScript, then are
  verified against the database
- **No direct database writes** are performed
- Repeating-rule edits are opened in Things rather than written to undocumented
  internal tables

> [!NOTE]
> This project is unofficial and is not affiliated with Cultured Code.

## Features

- Sidebar views: Inbox, Today, Upcoming, Anytime, Someday, Logbook, Trash,
  Deadlines, Repeating, and Projects
- Boolean filters for status, type, project, area, heading, tags, date ranges,
  overdue state, checklist, reminders, repeating items, and trash state
- Case-insensitive text search plus Rust-regex title/notes filters
- Stable UUID resolution by full id, 4+ character prefix, exact title, or unique
  title substring; ambiguous references return candidates instead of guessing
- Create, partial edit, complete, cancel, reopen, move between
  projects/areas/headings, and move to Trash
- Time-based reminders with strict `HH:MM` input
- Tag and area create/rename/delete with usage counts
- Bulk Things JSON import from a file or stdin, with per-item identity read-back
- Write verification based on pre-write snapshots and observed database state
- JSON and NDJSON output with stable error codes
- MCP over stdio with optional read-only mode and structured tool results
- AI-sized list results: notes are previewed to 200 characters, with full text
  available through `show` / `get`

## Install

Requires **macOS**, **Rust 1.89+**, and **Things 3**.

```sh
cargo install --path crates/things-cli
```

Or build a release binary:

```sh
cargo build --release -p things
./target/release/things --help
```

Homebrew (after a tagged release has published its checksum):

```sh
brew install --formula ./Formula/things.rb
```

## macOS permissions

1. Grant **Full Disk Access** to the terminal or app that runs `things`.
   The Things database lives under `~/Library/Group Containers`.
2. Open **Things → Settings → General** and enable **Things URLs**.
3. If you use `set_status`, `delete`, or tag mutations, allow automation access
   to Things when macOS prompts.

Run the built-in diagnostic:

```sh
things doctor
things doctor --json
```

## Quick start

```sh
# Read
things today --json
things search "quarterly" --json
things show <id> --json
things list --status open --tag focus --overdue --json

# Inspect a write without sending it
things add "Review proposal" \
  --when tomorrow --reminder 09:30 --deadline 2026-10-24 --tags work \
  --dry-run --json

# Write and verify
things add "Review proposal" --when tomorrow --reminder 09:30 --deadline 2026-10-24
things edit <id> --append-notes "Ask legal for feedback"
things edit <id> --reminder 08:45
things done <id>
things reopen <id>

# Tags, areas, and import
things tag add "deep-work"
things area add "Work"
things import ./payload.json
cat payload.json | things import -

# Destructive operations always require --yes
things delete <id> --yes
things empty-trash --yes
```

Use `--format ndjson` for streaming-friendly list output.
Use `--read-only` to disable every mutation for a process.

## MCP

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

Read-only agent:

```sh
things mcp --read-only
```

See [docs/MCP.md](docs/MCP.md) for the full tool reference.

## Import

```sh
things import payload.json
cat payload.json | things import -
things import payload.json --dry-run --json
```

The payload must be a JSON array in the format accepted by `things:///json`.

With verification enabled (the default), `things import --json` returns two
per-item arrays in payload order:

- `created` for every top-level or nested to-do/project
- `updated` for every `operation: update`

`confirmed` proves identity for creates, and covers status-only updates for
updates. Mixed-field updates stay explicitly unconfirmed. When an item is
unconfirmed, search for its title before retrying so a retry does not duplicate
it.

> [!TIP]
> Prefer `--dry-run --json` before a bulk import. Full verification semantics
> are documented in [docs/COMMANDS.md](docs/COMMANDS.md#import).

## Configuration

Config file: `~/.config/things/config.toml`

```toml
read_only = false

[output]
format = "text"   # text, json, or ndjson

[write]
verify_timeout_ms = 8000
verify_poll_ms = 250
create_missing_tags = false

[mcp]
read_only = false
default_page_size = 20
max_page_size = 100
```

Environment variables override the file:

| Variable | Purpose |
|---|---|
| `THINGSDB` | Explicit `main.sqlite` path |
| `THINGS_AUTH_TOKEN` | URL-scheme auth token |
| `THINGS_READ_ONLY` | `1` / `true` disables writes |
| `THINGS_VERIFY_TIMEOUT_MS` | Verification timeout |
| `THINGS_VERIFY_POLL_MS` | Verification poll interval |

If more than one `ThingsData-*` directory exists, database discovery refuses to
guess. Set `--db` or `THINGSDB`.

## Documentation

- [Command reference](docs/COMMANDS.md)
- [MCP setup and tool reference](docs/MCP.md)
- [Architecture and safety model](docs/ARCHITECTURE.md)
- [Compatibility and limitations](docs/COMPATIBILITY.md)
- [AI skill](skills/things/SKILL.md)

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Fixture tests do not require Things or personal data. Live read-only tests are
skipped when the Things database is unavailable.
