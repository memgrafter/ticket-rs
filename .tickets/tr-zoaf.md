---
id: tr-zoaf
status: open
open: true
deps: []
links: [tr-yeuy]
created: 2026-09-15T18:01:48Z
type: task
priority: 1
assignee: memgrafter
---
# Cross-project ticket crosslinks

# Cross-project ticket crosslinks

## Problem

`tk link` only works within a single store. Targets are resolved against the
local `.tickets` index (`storage.rs::resolve_id` -> `id.rs::resolve_id` over
the local index), so linking a ticket to one in another repo fails with
"ticket not found".

This comes up in practice: one feature often spans two repos. Example from
2026-09-15: the reasoning-work pair
`lglmp-jh1r` (loreblendr-gcp-llm-meter-proxy: retain per-model reasoning
settings until selectable reasoning) and
`byo-1lbk` (byollm iOS app: show reasoning in a folded bubble + settings).
They had to reference each other by hand-written text in the bodies because
`tk link` cannot cross the store boundary.

## Why full IDs are already safe to scope

`generate_id(&dir_name)` prefixes IDs with the repo's directory name
(`lglmp-`, `byo-`, `tr-`), so a full ID is globally unique across projects.
A cross-project link is unambiguous *given* the ID; what's missing is the
ability to express "this ID lives in another store" and to resolve it.

## What to build

1. **Scoped link syntax.** A link target that names another store/project.
   Candidate forms (pick one, keep it simple):
   - `--project <path-or-name> <id>` on `tk link`, or
   - `project:<id>` / `@<project>:<id>` inline target syntax.
   The stored `links` entry should carry the project so `show`/`ls` can mark
   it as external.
2. **Resolution.** When linking, if the target isn't in the local store and
   is scoped, resolve against the named store (path, or a project-registry
   mapping — see tr-yeuy for the default-mode per-project store direction;
   the two should not fight). No hard dependency on the other repo being
   checked out at link time? Decide: validate-at-link (fail if store not
   found) vs. allow-dangling (store the scoped id, validate on view).
3. **Display.** `tk show` / `tk ls` render external links distinctly
   (e.g. `@byollm:byo-1lbk`) and don't count them as local deps in
   `ready`/`blocked` (links already don't; confirm `link`/`dep` stay
   distinct — a cross-project DEP is a different, harder question, out of
   scope here).
4. **Query.** `query` JSONL should carry the scope so tooling can filter
   external links.
5. Tests: link within local store unchanged; scoped link to a second store
   in a fixture dir; scoped link display; dangling-scoped behavior per the
   chosen policy.

## Notes

- 2026-09-15: filed because the reasoning feature pair (lglmp-jh1r /
  byo-1lbk) needed a cross-repo crosslink and had to fall back to prose
  references in each ticket body.
- Related: tr-yeuy (default mode: store tickets in ~/.tk by project ID with
  manifest mapping) — a project registry there is the natural home for
  resolving bare project names in link targets.
