# ticket-rs

Git-backed ticket system. Markdown `.tickets/<id>.md` with YAML frontmatter. Types auto-generated from `ticket.ts`.

## CLI Commands

| Cmd | Args | Behavior |
|---|---|---|
| `create [title]` | `-d, --design, --acceptance, -t, -p, -a, --external-ref, --parent, --tags` | Prints ID. Defaults: task, P2, assignee=git user.name |
| `status <id> <s>` | `open\|in_progress\|closed` | `start`, `close`, `reopen` aliases |
| `dep\|undep <id> <dep>` | | Asymmetric blocking. Both IDs validated/resolved |
| `link\|unlink <id> <id>...` | `--all-pairs` (alias `--mesh`) | Symmetric, stored in both files. Star by default: first id paired with each other id. `--all-pairs` = every pair. Prints each pair added/removed; already-linked (or not-linked) pairs reported, not duplicated. Self-link errors |
| `ls` | `--status, -a, -T, --tags, -s` | Filters + full-text search on title. Sort: priority asc, ID asc |
| `ready\|blocked` | `-a, -T` | Ready = all deps closed. Blocked = ≥1 open dep |
| `closed` | `--limit=N` (20) | Recently closed, sorted by created DESC |
| `show <id> [section]...` | `--short, --raw` | File verbatim + computed Blockers/Blocking/Children/Linked. Sections: any `##`-`######` heading, case-insensitive, with subsections; reserved names frontmatter, title, description, blockers... Unknown = exit 1 listing headings. `--short` = frontmatter + title + computed. `--raw` = cat |
| `add-note <id> [text]` | | Timestamped `**<iso>**`. Reads stdin if no text arg |
| `query [.field==\"val\"]` | | JSONL; `body` = stored markdown after frontmatter. Field filter: status, type, priority, assignee |
| `dep-tree <id>` | `--full` | Box-drawing. Dedup by default (--full shows all) |
| `dep-cycle` | | DFS cycle detection on open tickets only |
| `edit <id>` | | Prints file path (no $EDITOR launch) |
| `queue serve` | `--port` (7878), `--data` | HTTP on 127.0.0.1 only; append-only NDJSON log (default `~/.tickets/_global_queue.jsond`), replayed at start; file lock = one server per log. Indexes: queue -> entries, queue -> events, (queue, repo, group) -> raise |
| `queue raise <title>` | `--repo, --queue, --group, -d, -t, -p, -a, --tags` | Raise entry (`q-<8 alnum>`, state pending). Same queue+repo+group = occurrence (`count`, `first_seen`, `last_seen`, `latest`). No `.tickets` needed |
| `queue poll` | `--queue, --since, --once, --interval, --exec, --no-materialize, --cursor` | JSONL changes after cursor. With `.tickets` found: claim + create its repo's pending raises first (`external-ref: queue:<raise id>`); for its own group tickets, one note per poll for new occurrences (`noted` tracks them), reopening closed ones |
| `queue ls\|show` | `--queue, --repo, --kind, --json` | Current entries |

## Modules

| File |  Responsibilities |
|---|---|
| `main.rs` | Clap derive dispatch, all `cmd_*()` functions |
| `types.rs` | **Auto-generated from ticket.ts** via `generate-types.sh` |
| `storage.rs` | `.tickets/` CRUD, parent-dir walking, YAML serde, search engine |
| `id.rs` | ID gen (`dir-prefix-4alphanum`), partial resolution (exact→suffix→prefix→substring) |
| `graph.rs` | `DependencyGraph` with deps/dependents maps, DFS cycle detection, subtree depth, tree render |
| `display.rs` | Human-readable show/ls/blocked + JSONL serialization |
| `queue.rs` | Queue server state/routes, client commands, `mirror()` called after every ticket change |
| `http.rs` | Minimal HTTP/1.1 client + request parsing on `std::net` (no HTTP crate) |

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

- **Dir resolution**: walks parents for `.tickets/`, skipping `$HOME/.tickets` (queue log dir). `TICKETS_DIR` env var overrides. `create` inits `.tickets/` in cwd (refused in `$HOME`)
- **Status lifecycle**: open → in_progress → closed (reopen → open)
- **Type enum**: bug, feature, task (default), epic, chore
- **Partial ID resolution**: exact match → suffix → prefix → substring. Errors on ambiguous
- **Sort order**: priority ASC (0 highest), then ID ASC
- **Tags filter**: intersection (all specified tags must match)
- **Deps are blocking**: A depends on B means B blocks A
- **Queue is opt-in**: no `TK_QUEUE_URL` = no network. Push failure = stderr warning, exit 0. Key = repo (dir holding `.tickets`, or `TK_QUEUE_REPO`) + id. Queue name: `TK_QUEUE_NAME`, else the entry's current queue, else `default`
- **Queue server safety**: bind 127.0.0.1 only (no flag); reject non-loopback `Host`; POST needs `X-TK-Queue: 1`

## Dev

```sh
./test.sh          # unit tests (quick)
./test.sh --all    # unit + integration + clippy
./generate-types.sh  # regenerate src/types.rs from ticket.ts
cargo build -r     # release: target/release/ticket-rs
```

Tests: 150 total (48 unit + 88 integration + 14 queue in `tests/queue.rs`, which start `tk queue serve --port 0`).

Setup (macOS): `setup/install.sh` (release build, atomic install to `~/.local/bin/tk`), `setup/queue.sh install|uninstall|status` (launchd agents from `setup/launchd/`: server, ssh tunnel to vert; logs `~/Library/Logs/tk/`). Integration tests find the binary via `CARGO_BIN_EXE_tk`, so `CARGO_TARGET_DIR` works.