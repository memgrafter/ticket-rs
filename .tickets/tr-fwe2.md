---
id: tr-fwe2
status: open
open: true
deps: []
links: []
created: 2026-08-17T22:57:10Z
type: feature
priority: 2
assignee: memgrafter
tags: [states, cli]
---
# Split status into a free-form `status` string + a required `open` boolean

`status` was a closed enum (`open` | `in_progress` | `closed`) where `closed`
doubled as both "no longer active" and "how it ended". That conflates two
orthogonal ideas. This splits them, and applies the same free-form treatment to
`type`.

## The model

- **`open: bool`** — the single source of truth for open/closed. `true` =
  active (open / in progress), `false` = terminal (closed / done / cancelled).
  **Required** on every ticket.
- **`status: string`** — a free-form label. The tool stores it verbatim and does
  not interpret it; the user assigns meaning in their own analytics
  (`done`, `cancelled`, `in-progress`, …).
- **`type: string`** — also free-form now (conventional: bug, feature, task,
  epic, chore — but any string is accepted).

## Source of truth

`ticket.ts` is canonical: `Metadata.open: boolean`, `Metadata.status: string`,
`Metadata.type: string`; `Filter`/`CreateOptions` updated; `open` added to
`CANONICAL_FIELD_ORDER`. `src/types.rs` is regenerated from it via
`generate-types.sh` (the `Status`/`TicketType` enums are gone).

## Behavior

- **Read gate:** parsing a ticket with no `open:` line is a hard error:
  `ticket '<id>' is missing the required 'open' field. Run \`tk migrate\` to add
  it, then retry.` Every read command (`ls`, `show`, `query`, `ready`, …) is
  gated on this until you migrate.
- **`tk migrate`** (no args): adds `open:` to every ticket in the resolved
  `.tickets/` dir that lacks one. Value is `false` when `status` is `closed`,
  else `true`. Idempotent (already-migrated files are skipped).
- **Lifecycle verbs** set `open` (and a conventional `status`):
  - `start` → `open: true`, `status: in_progress`
  - `close` → `open: false`, `status: closed`
  - `reopen` → `open: true`, `status: open`
  - `status <id> <val>` → sets the free-form `status` label only (no validation,
    does not touch `open`).
- **Graph logic** keys off `open`: `ready`/`blocked`/`dep-cycle` operate on
  `open: true` tickets; a dep is satisfied when its `open` is `false`.
- **`closed` command** lists `open: false` tickets (all terminal states).
- **`ls` filters:** `--status <str>` (free-form label), plus new `--open` /
  `--closed` (the boolean view). No backwards-compat for the old enum meaning.
- **`query`** supports `.open == true|false` in addition to `.status`/`.type`.
- **`create`** writes `open: true`, `status: open`, `type: task` by default.

## Tests

34 unit + 69 integration tests, clippy clean. New coverage: `migrate` (adds
field, idempotent), missing-`open` error, `--open`/`--closed` filters, free-form
`status`.
