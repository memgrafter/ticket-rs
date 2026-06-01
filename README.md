# ticket-rs

> You've already got the best issue tracker in the world: your git repo.  
> Everything else is just a UI bolted on top of the same damn data model.

**ticket-rs** keeps your tickets as plain markdown files next to your code — no database, no API, no login screen, no "we'll migrate you to our new platform next quarter." Track issues, deps, and blockers from your terminal with `tk create`, `tk dep`, `tk ready`, and get back to shipping.

---

## 5 Whys

### Why tickets?

We need a place to organize work for the project. Written down, shared, ordered.

### Why markdown files?

*"Ok, we need tickets. Why not put them in a database or a SaaS tool?"*

Because every database needs a special client to read and write. A SaaS tool needs a login, a page load, and a network request. A markdown file doesn't need anything — it's just text. Works with any editor, any terminal. No setup, no login, no vendor, no network calls.

### Why stored in git?

*"Ok, markdown files work — but why keep them in the repo specifically? They could just sit in a shared folder."*

Because tickets describe work that changes the code. If they're in a shared folder, they're disconnected — someone updates a ticket, someone else merges code, and nobody connects the two. In the repo, ticket changes are in the same commits as code changes. `git log` shows both. `git checkout` gives both. They don't drift because they can't drift.

### Why a CLI?

*"Great, so there's a `.tickets/` folder in the repo. Why not just edit the files directly?"*

A CLI gives coding agents, humans, and scripts a stable, discoverable interface. An agent doesn't know how to find the right YAML field and edit it in place — but it can run `tk close abc-123`. CI doesn't want to parse markdown frontmatter — it can run `tk query '.status == "closed"'` and get JSONL back. Tab completion discovers every command. `--help` documents every flag. The files are the source of truth, but the CLI is the API.

### Why not use an existing CLI?

*"There are a dozen issue trackers with CLIs. Linear has one. GitHub has `gh`. Why build another?"*

Because every existing CLI assumes network access and an account on their platform. `gh issue create` requires authentication, API rate limits, and a working internet connection. This project's tickets are local files — they work in an offline checkout, in a CI container with no egress, in a `git clone` on a plane. The CLI is a thin wrapper over local files. No API, no auth, no dependency on an external service. It works when everything else is down.

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

## Credits

- `ticket.ts` and interface based on https://github.com/wedow/ticket
