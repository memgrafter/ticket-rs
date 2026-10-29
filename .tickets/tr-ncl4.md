---
id: tr-ncl4
status: closed
open: false
deps: []
links: []
created: 2026-10-28T17:33:49Z
type: bug
priority: 1
assignee: memgrafter
tags: [link, cli, ux]
---
# tk link with 3+ ids links every pair (full mesh), silently; linking one ticket to several others adds unwanted links between the others

## Problem

`tk link A B C D` links every pair of the given tickets (a full mesh), not A to each of B, C, D. Nothing in the help or the output warns about this, and there is no undo for a batch.

Code: `cmd_link` (`src/main.rs:390`) loops over every ticket i and adds every other ticket j (i != j) to its `links`. The help text only says "Link tickets together (symmetric)" (`src/main.rs:91`).

## What happened (2026-10-25, loreblendr-gcp-llm-meter-proxy)

The intent was to link a new ticket (lglmp-j53t) to 7 existing tickets. `tk link lglmp-j53t <7 ids>` printed "Added 50 link(s) between 8 tickets": 7 wanted links (14 directed entries) plus 18 unwanted links between the old tickets. The old tickets were fixed by hand-editing each `links:` line back to its git version plus the new id. `tk unlink` takes one pair at a time, so undoing it with tk would have taken 18 separate calls.

## Expected

The common case (link one ticket to several others) should be the default, or be one clear flag away:

- `tk link <id> <id>...` links the first id to each of the others (star). A full mesh needs an explicit `--all-pairs` (or `--mesh`).
- The output names the pairs it added (`lglmp-j53t <-> lglmp-jwwv`, ...), so a wrong batch can be seen right away.
- Optional: `tk unlink <id> <id>...` with the same star shape, so a batch can be undone in one call.

## Compatibility

Changing the default changes behaviour for anyone who relies on the mesh. Check the repo's scripts and docs (README.md, SKILL.md, tests/) for callers that pass more than 2 ids before changing it.

## Notes

**2026-10-29T03:32:31Z**

Fixed on branch fix/tr-ncl4-link-star (not merged).

- `tk link <id> <id>...` now links the first id to each of the others (star). The old full mesh needs `--all-pairs` (alias `--mesh`).
- Output names every pair: `Added link: A <-> B`, `Already linked: A <-> B` for pairs that already existed (no duplicate entries written), and a summary line `Added N link(s); M already existed` when more than one pair is involved. The count is pairs, not directed entries.
- `tk unlink <id> <id>...` takes the same shape (and `--all-pairs`), so a batch is undone in one call. Prints `Removed link: A <-> B` or `Not linked: A <-> B`. The 2-id form works as before.
- Linking a ticket to itself (`tk link a a`, or the hub repeated later) is an error; repeated other ids are dropped.
- Help text, README.md, SKILL.md, AGENTS.md updated. Unit tests for the pair builder and integration tests for star, --all-pairs/--mesh, already-linked, multi-unlink, 2-id link/unlink, self-link.
- Integration tests now locate the binary with `CARGO_BIN_EXE_tk` instead of a hardcoded `target/debug/tk`, which ran a stale binary when CARGO_TARGET_DIR is set.
- Callers passing 3+ ids: none in this repo's scripts/tests/docs. Outside it, loreblendr-gcp-llm-meter-proxy/AGENTS.md says "`tk link A B C` links symmetrically" (prose, now means A-B and A-C).
