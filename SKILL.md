---
name: ticket-rs
description: 'File-based ticket system — create, track, block, and query tickets as markdown files in .tickets/. One binary, no setup, works offline, lives in the repo.'
---

When you need to track work items, dependencies, or blockers without leaving the terminal. Tickets are `.tickets/<id>.md` files with YAML frontmatter — same repo, same git history, no external service.

## Why use it

- **In-repo, no drift** — ticket changes are in the same commits as code changes. `git log` shows both. They can't get out of sync.
- **Zero setup for the agent** — `tk create "thing"` just works. No env vars, no config file, no auth.
- **Works offline / in CI** — no network calls, no API keys, no rate limits.
- **Dependency-aware** — `tk ready` tells you what's actionable right now. `tk blocked` shows what's stuck. `tk dep-tree` renders the graph.
- **JSONL output for scripts** — `tk query '.status == "open"'` pipes cleanly into `jq`, CI checks, or another agent.

## When to use

- You're about to implement something and want a ticket created first (so the commit references it).
- You need to block work on a dependency and want `tk ready` / `tk blocked` to reflect it.
- An agent in another window is working on something you depend on — `tk dep` captures the relationship explicitly.
- You need CI to check that nothing is blocked before merging, or that nothing has open deps.
- You want to query tickets by field (status, type, priority, assignee) from a script or agent.

## When NOT to use

- You need real-time multi-user collaboration — these are files, not a live document.
- You need a web UI, email notifications, or image attachments.
- The project already has a dedicated issue tracker and you'd be duplicating.

## Commands

| What | How |
|---|---|
| Create a ticket | `tk create "Title" [-t type] [-p priority] [-a assignee] [--tags x,y] [-d description]` |
| Change status | `tk start <id>` / `tk close <id>` / `tk reopen <id>` (or `tk status <id> <s>`) |
| Block / unblock | `tk dep <id> <dep_id>` / `tk undep <id> <dep_id>` |
| Symmetric link | `tk link <id> <other_id>` / `tk unlink <id> <other_id>` |
| List / filter | `tk ls [--status S] [--tags x,y] [-s "search"]` |
| What's actionable | `tk ready` / `tk blocked` |
| Recently closed | `tk closed [--limit N]` |
| Full view | `tk show <id>` |
| Add a note | `tk add-note <id> "text"` |
| Query (JSONL) | `tk query ['.field == "val"']` |
| Dep tree | `tk dep-tree <id> [--full]` |
| Detect cycles | `tk dep-cycle` |
| Help | `tk --help` / `tk <cmd> --help` |

## Example session

```sh
# You're about to implement dark mode. Create the ticket first.
tk create "Add dark mode" -t feature -p 1 -a alice --tags ui,ux
# → project-a1b2

# Create the prerequisite.
tk create "Define color tokens"
# → project-c3d4

# Dark mode depends on tokens being done.
tk dep project-a1b2 project-c3d4

# Now check: what's actionable?
tk ready
# → project-c3d4 (dark mode is blocked)

# After tokens are done, close and re-check.
tk close project-c3d4
tk ready
# → project-a1b2 is now unblocked

# When CI checks before a merge, it runs:
tk query '.status == "open" and .deps != []'
# → lists anything that shouldn't be merge-blocking
```

## How it works

`.tickets/` folder in the project root. Each ticket is `<id>.md` with YAML frontmatter. `tk` walks parent dirs to find it. No database, no server, no env setup — just a binary on `$PATH`.

**Cost**: One `cargo build --release` + copy binary. **Benefit**: Tickets in the repo, work offline, zero setup for any agent or script.