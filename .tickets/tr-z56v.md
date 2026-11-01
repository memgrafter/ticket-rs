---
id: tr-z56v
status: open
open: true
deps: []
links: []
created: 2026-11-01T15:24:28Z
type: feature
priority: 1
assignee: memgrafter
tags: [queue, network, poll]
---
# tk queue: networked ticket queue with poll-based clients (create/update mirror, raise from other hosts)

Owner 2026-11-01 (from the characteragent.ai-social-content session, csc-vt6o): "I want some kind of networked queue for the tickets too, so when we create the ticket we also drop it in the queue, and whenever we work on a ticket, we search the queue ticket ids for a matching ticket and update the ticket in the queue. We can have a better distributed system later, then we want to attach this session to check the queue. ... I am partial to rewriting the parts we need into the existing tk which could have a listener on a socket, I already have a design for that that lets the os wake, it's not as good on macos. This is too annoying for now, so let's actually poll the queue in tk cli which gives us a poll based runtime execution."

Scope now: POLLING ONLY. No socket listener / OS wake (the owner has a separate design for that; later). No external broker.

## Why
Non-LLM batch scripts (first user: the TRELLIS.2 3D batch runner on host vert, Linux) must raise exceptions as tk tickets, and an agent session on the Mac (macOS) must notice them by polling. Today tickets are files in one git repo on the Mac; vert has no clone.

## Requirements
1. A queue reachable from both hosts (Mac and vert over the LAN/ssh). Entries are keyed by ticket id (prefix + id; also record the repo), so it is an upsert mirror plus a change log, not a fire-and-forget FIFO.
2. `tk create` also puts the new ticket in the queue (when a queue is configured; works offline without one, as today).
3. Every command that changes a ticket (status/start/close/reopen, add-note, dep/undep, link/unlink, edit when detectable) finds the matching id in the queue and updates that entry.
4. A way for a host with no .tickets dir (vert) to raise a ticket into the queue (e.g. `tk raise ...` or `tk create --queue-only`), which a poller on the repo host turns into a real .tickets file.
5. `tk queue poll [--since <seq>] [--once] [--interval N]`: prints new/changed entries (JSONL), keeps its cursor; optional `--exec <cmd>` runs a command per entry (the "poll-based runtime execution"). An agent session can run it from a cron or a monitor.
6. Simple ops: `tk queue serve` (if a server is chosen), `tk queue ls`, `tk queue show <id>`, config by env or a small config file (no secrets in tickets).

## Recommendation from the main session (to evaluate, not binding)
tk-native, not an external broker: the volume is tiny (tens of items a day) and polling is fine. Two shapes to compare and pick one, with reasons in this ticket:
 (a) `tk queue serve`: a small HTTP server in tk with SQLite or append-only JSONL storage; bind 127.0.0.1 by default; remote hosts reach it through an ssh tunnel (no open LAN port, no auth system needed).
 (b) no server: a spool directory on one host, reached over ssh by the client (atomic rename for writes).
If we outgrow this: an external free-software broker (e.g. NATS JetStream, Valkey streams, beanstalkd); check their licences from the sources before recommending any.

## Rules
- Do not replace the installed binary ~/.local/bin/tk (many agents and sessions use it); build and test in this repo only; installing needs the owner's approval.
- Keep current behaviour unchanged when no queue is configured (all existing tests pass).
- Tests for the new commands (integration tests in tests/), including a two-client test and failure cases (server down -> create still works and says the queue push failed).
- Update README/AGENTS.md/SKILL.md for the new commands. Small focused commits; never git add -A; never push.

