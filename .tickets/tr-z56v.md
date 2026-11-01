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

## Notes

**2026-11-01T15:27:58Z**

Decision (agent, 2026-11-01): option (a) `tk queue serve`, a small HTTP server inside tk with an append-only JSONL log. Tentative until the owner reviews; nothing is installed.

Comparison (checked: vert has curl, wget, python3, socat, nc, flock; no tk, no cargo. The Mac reaches vert with `ssh vert`; vert -> Mac ssh is not known to work):

| | (a) HTTP server + JSONL log | (b) spool dir over ssh |
|---|---|---|
| Change sequence | one process assigns seq under one lock: simple, total order | needs a cross-host lock; macOS has no `flock` CLI; time-named files reorder with clock skew |
| vert raises with tools it has | `curl --data-urlencode` through an ssh tunnel; no JSON escaping in bash | shell `printf` + `mv`; the script must write valid JSON or a custom format |
| Mac `tk create`/update cost | one loopback HTTP call; fails fast (connection refused) when down | one ssh per command if the spool is remote (slow, can hang without ConnectTimeout) |
| Long-running pieces | server + one `ssh -N -R` tunnel (both started from the Mac) | only the poller |
| vert reads entry state (did my raise become a ticket?) | `curl /entry?...` | `ssh`/`cat` of files |
| New deps | serde_json (already in Cargo.lock as a transitive dep) | none for writing; serde_json or a hand parser for reading |
| Tests | spawn `tk queue serve --port 0` on 127.0.0.1 | need an ssh stand-in |

Dominant criterion: one total order of changes (the poll cursor depends on it). Tie-breaker: vert can raise with curl and no JSON escaping. Accepting: a server and a tunnel must be kept running; when they are down, `tk create` still works and prints a warning, and vert's curl fails (its script must retry or spool).

Shape:
- Server: `tk queue serve [--port 7878] [--data FILE]`, binds 127.0.0.1 only (no flag to change it). Storage: append-only JSONL, one line per change (full entry snapshot + seq); state rebuilt by replay at start. One request at a time, socket timeouts, body size cap. No auth: anyone who can reach 127.0.0.1:port on the Mac or on vert (via the tunnel) can write.
- Client config: env `TK_QUEUE_URL` (e.g. http://127.0.0.1:7878). Unset = today's behaviour, no network. Optional `TK_QUEUE_REPO` overrides the repo name (default: name of the dir that holds .tickets).
- Mirror: create/start/close/reopen/status/add-note/dep/undep/link/unlink push a snapshot of each changed ticket (key = repo + id). Push failure = warning on stderr, exit 0.
- Raise: `POST /raise` (form or JSON: repo, title, description, type, priority, tags, assignee, host) and `tk queue raise` for hosts with tk. The poller in the matching repo claims the raise, creates the ticket (external-ref `queue:<raise id>` keeps it idempotent), and the raise entry records the new ticket id.
- `tk queue poll [--since SEQ] [--once] [--interval N] [--exec CMD] [--no-materialize]`: JSONL events, cursor file under ~/.local/state/tk (override with --cursor or TK_QUEUE_STATE_DIR). `tk queue ls`, `tk queue show <id>`.
- HTTP client and server are hand-written on std::net (small HTTP/1.1 subset, Connection: close) to avoid an HTTP crate.

New crate: serde_json 1.0.x (licence "MIT OR Apache-2.0", from its Cargo.toml). Its deps itoa ("MIT OR Apache-2.0"), memchr ("Unlicense OR MIT"), serde_core ("MIT OR Apache-2.0") and zmij are already in Cargo.lock; zmij licence to be read from its Cargo.toml after fetch.
