# ticket-rs

Git-backed ticket system. Markdown `.tickets/<id>.md` with YAML frontmatter. Types auto-generated from `ticket.ts`.

## CLI Commands

| Cmd | Args | Behavior |
|---|---|---|
| `create [title]` | `-d, --design, --acceptance, -t, -p, -a, --external-ref, --parent, --tags` | Prints ID. Defaults: task, P2, assignee=git user.name |
| `status <id> <s>` | `open\|in_progress\|closed` | `start`, `close`, `reopen` aliases |
| `dep\|undep <id> <dep>` | | Asymmetric blocking. Both IDs validated/resolved |
| `link <ids...>` | `unlink <id> <target>` | Symmetric. Stored in both files |
| `ls` | `--status, -a, -T, --tags, -s` | Filters + full-text search on title. Sort: priority asc, ID asc |
| `ready\|blocked` | `-a, -T` | Ready = all deps closed. Blocked = ≥1 open dep |
| `closed` | `--limit=N` (20) | Recently closed, sorted by created DESC |
| `show <id>` | | Enhanced: blockers/blocking/children/linked sections |
| `add-note <id> [text]` | | Timestamped `**<iso>**`. Reads stdin if no text arg |
| `query [.field==\"val\"]` | | JSONL. Field filter: status, type, priority, assignee |
| `dep-tree <id>` | `--full` | Box-drawing. Dedup by default (--full shows all) |
| `dep-cycle` | | DFS cycle detection on open tickets only |
| `edit <id>` | | Prints file path (no $EDITOR launch) |

## Modules

| File |  Responsibilities |
|---|---|
| `main.rs` | Clap derive dispatch, all `cmd_*()` functions |
| `types.rs` | **Auto-generated from ticket.ts** via `generate-types.sh` |
| `storage.rs` | `.tickets/` CRUD, parent-dir walking, YAML serde, search engine |
| `id.rs` | ID gen (`dir-prefix-4alphanum`), partial resolution (exact→suffix→prefix→substring) |
| `graph.rs` | `DependencyGraph` with deps/dependents maps, DFS cycle detection, subtree depth, tree render |
| `display.rs` | Human-readable show/ls/blocked + JSONL serialization |

## Data Model (from ticket.ts)

```
Ticket { Metadata + Data }
  Metadata: id, status, deps[], links[], created, type, priority, assignee?, externalRef?, parent?, tags?[],
  Data: title, description?, design?, acceptance?, notes?[Note{timestamp, body}]
```

## File Format

```
---
id: nw-5c46
status: open
deps: [nw-0001]
links: [nw-0002]
created: 2024-01-15T10:00:00Z
type: task
priority: 2
assignee: Alice
external-ref: gh-123
parent: nw-0000
tags: [ui, backend]
---
# Title
## Design / ## Acceptance Criteria / ## Notes
```

## Key Rules

- **Dir resolution**: walks parents for `.tickets/`. `TICKETS_DIR` env var overrides. `create` inits `.tickets/` in cwd
- **Status lifecycle**: open → in_progress → closed (reopen → open)
- **Type enum**: bug, feature, task (default), epic, chore
- **Partial ID resolution**: exact match → suffix → prefix → substring. Errors on ambiguous
- **Sort order**: priority ASC (0 highest), then ID ASC
- **Tags filter**: intersection (all specified tags must match)
- **Deps are blocking**: A depends on B means B blocks A

## Dev

```sh
./test.sh          # unit tests (quick)
./test.sh --all    # unit + integration + clippy
./generate-types.sh  # regenerate src/types.rs from ticket.ts
cargo build -r     # release: target/release/ticket-rs
```

Tests: 84 total (36 unit + 48 integration). All passing, clean clippy.