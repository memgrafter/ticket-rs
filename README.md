# ticket-rs

> You've already got the best issue tracker in the world: your git repo.  
> Everything else is just a UI bolted on top of the same damn data model.

**ticket-rs** keeps your tickets as plain markdown files next to your code — no database, no API, no login screen, no "we'll migrate you to our new platform next quarter." Track issues, deps, and blockers from your terminal with `tk create`, `tk dep`, `tk ready`, and get back to shipping.

---

## 5 Whys

### Why tickets in markdown?

Because SQL schemas rot. When your ticket system is `cat .tickets/*.md | grep "status: open"`, there's nothing to migrate. No schema versioning. No vendor lock-in. Your tickets live in the repo, move with the repo, and diff like everything else.

### Why a CLI instead of a web UI?

Because web UIs optimize for the 5% case — rich text editors, drag-and-drop boards, emoji reactions — and neglect the 95%: create, status, dependency, done. `tk` gives you the 95% in under 100ms with tab completion and `git log` integration.

### Why git-backed at all?

Because **ticket state should follow code state**. When you `git checkout feature/foo`, the tickets for that feature should be right there. Code review includes the ticket. The CI pipeline can read `tk query '.status == "closed"'`. Everything stays in sync because it *can't* get out of sync.

### Why dependency tracking in a ticket system?

Because every real project has a critical path. `tk dep-tree` shows you exactly what's blocking what. `tk ready` tells you what you can pick up right now. `tk dep-cycle` catches circular reasoning before it ships. Spreadsheets don't do this. JIRA does it inside a modal inside a dashboard inside a ten-second load.

### Why Rust?

Because a ticket system that takes 500ms to print "hello" is a ticket system you stop using. `ticket-rs` starts in under 10ms. Also: no runtime, no VM, one binary.

---

## Install

```sh
cargo build --release
```

Copy the binary somewhere on your `$PATH`:

**Linux / macOS** (same command works for both):
```sh
cp target/release/ticket-rs ~/.local/bin/tk    # or /usr/local/bin/tk
```

Verify:
```sh
tk --version    # ticket-rs 0.1.0
```

---

## Quickstart — CRUD a project by hand

```sh
cd my-project

# Create a ticket (prints ID like "my-project-a1b2")
tk create "Fix login timeout"

# Create with type, priority, assignee, tags
tk create "Add dark mode" -t feature -p 1 -a alice --tags ui,ux

# Create with a description and acceptance criteria
tk create "Payment flow" -d "Handle Stripe webhooks" --acceptance "Refunds work"

# Set status
tk start my-project-a1b2
tk close my-project-a1b2
tk reopen my-project-a1b2
tk status my-project-a1b2 in_progress

# Dependencies (asymmetric blocking)
tk create "Write tests"
tk dep my-project-a1b2 my-project-c3d4

# Links (symmetric)
tk link my-project-a1b2 my-project-c3d4
tk unlink my-project-a1b2 my-project-c3d4

# List, filter, search
tk ls
tk ls --status open --tags ui -s "login"

# See what's actionable
tk ready                    # open tickets with all deps closed
tk blocked                  # open tickets with open deps

# Recently closed
tk closed --limit 5

# Show / edit
tk show my-project-a1b2
tk edit my-project-a1b2     # prints file path

# Add notes
tk add-note my-project-a1b2 "Found the root cause"

# JSONL querying
tk query
tk query '.status == "open"'

# Dependency tree
tk dep-tree my-project-a1b2
tk dep-tree my-project-a1b2 --full
tk dep-cycle
```

Tickets live in `.tickets/<id>.md` — open them in any editor, commit them with `git add .tickets && git commit`.

---

## Shell integration (recommended)

You can find tickets by partial ID — suffix, prefix, or substring:

```sh
tk show a1b2       # exact match first
tk show a1         # prefix match
tk show timeout    # substring match
```

> **Note:** ambiguous partial IDs (multiple matches) produce an error.

---

## Data model

Every ticket is a markdown file with YAML frontmatter:

```yaml
---
id: project-a1b2
status: open
deps: [project-c3d4]
links: []
created: 2025-06-01T12:00:00Z
type: feature
priority: 1
assignee: alice
tags: [ui, ux]
---
# Add dark mode
## Design
Use CSS custom properties. No JS.
## Acceptance Criteria
- Toggle in settings persists to localStorage
- All surfaces covered: nav, editor, modals
## Notes
**2025-06-01T14:30:00Z**
Started on the color palette.
```

Commit these files. Diff them. Review them. They're just text.

---

## All commands

| Command | Args | What it does |
|---|---|---|
| `create [title]` | `-d, --design, --acceptance, -t, -p, -a, --external-ref, --parent, --tags` | Creates a ticket, prints ID |
| `status <id> <s>` | `open\|in_progress\|closed` | Manual status transition |
| `start` / `close` / `reopen` | `<id>` | Shorthands for common transitions |
| `dep <id> <dep_id>` | | Add dependency (blocking) |
| `undep <id> <dep_id>` | | Remove dependency |
| `link <ids...>` | | Symmetric link |
| `unlink <id> <target>` | | Remove link |
| `ls` | `--status, -a, -T, --tags, -s` | List with filters + full-text search |
| `ready` | `-a, -T` | Tickets ready to work on |
| `blocked` | `-a, -T` | Tickets waiting on deps |
| `closed` | `--limit=N` (20), `-a, -T` | Recently closed |
| `show <id>` | | Full ticket display with deps/links |
| `edit <id>` | | Prints file path |
| `add-note <id> [text]` | | Timestamped note (stdin if no text) |
| `query [.field=="val"]` | | JSONL output with optional filter |
| `dep-tree <id>` | `--full` | Box-drawing dependency tree |
| `dep-cycle` | | Find cycles (open tickets only) |

---

## Config / Directory resolution

`tk` looks for `.tickets/` by walking up parent directories. Override with `TICKETS_DIR` env var. Run `tk create` from a project root to initialize `.tickets/` there — works from any subdirectory.