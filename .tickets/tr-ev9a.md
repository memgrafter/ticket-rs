---
id: tr-ev9a
status: open
open: true
deps: []
links: []
created: 2026-11-01T16:19:13Z
type: feature
priority: 1
assignee: memgrafter
tags: [queue]
---
# tk queue v2: named queues, Sentry-style raise groups, ~/.tickets/_global_queue.jsond, launchd setup, install

Owner 2026-11-01 ~08:15 (verbatim, answers to tr-z56v's open decisions):
"1. yes 2. mac 3. yeah put these jobs in the repo in install/ setup/ or whatever already exists, maybe a setup.sh script or scripts/setup or bin/setup you tell me. 4. ok good, put it in my .bashrc on vert,argent, and the mac... btw the queue might to get pretty big, we will have to work on some batch drain mechanism or something. we will definitely need named queues so lets start with tk just accepting an arbitrary key for queue name and we will let the queue be filtered by the name for now, since having multiple queues is more of a process isolation question, the data isolation is O(logn) for filter right with a practical expectation closer to O(1) taking into account this scale.
6. i dont think duplicate key for raises is quite right, this reminds me of sentry exception flow now that we are building it. it's more like... we need to group the raises, and if we're worried about queue flooding we should be handling raises better in the batch, e.g. have some raise awareness in the batch processor, and with too many raises we pause the batch. that's a client side concern. keeping the queue safe is still a tk concern, so we should use persistence for queue items, not memory queue. I think ~/.tickets/_global_queue.jsond is good. We would load for reads separate from writes so that during a read we just don't have any updated items and on write we could notify an active read, or read could watch the file, that there are new items. Or defer that."

Decisions (owner): install to ~/.local/bin/tk = yes; server runs on the Mac; launchd jobs for the server and the vert tunnel live in this repo; TK_QUEUE_URL in the shell profiles of the Mac, vert and argent; no dedupe key — raises are GROUPED (Sentry-like); queue persisted on disk at ~/.tickets/_global_queue.jsond.

Work:
1. Named queues: every entry/raise carries a free-form queue name (`--queue NAME`, default "default"; HTTP param `queue`); ls/show/poll filter by name. Index entries by name in the server (name -> entries) so a filtered read is O(1) lookup + O(k) results (today handle_entries scans all entries: O(n)).
2. Raise groups (Sentry model: group = issue, raise = event): a raise may carry `group` (fingerprint). The first raise of a (repo, queue, group) creates the entry/ticket; later raises in that group add an occurrence (count, first_seen, last_seen, latest summary/evidence) to the same entry and append a short note to the same ticket (not a new ticket). A closed group that raises again reopens or is flagged (choose; write why in the ticket). No group => one ticket per raise (as today).
3. Storage path ~/.tickets/_global_queue.jsond (owner's exact name; format = newline-delimited JSON). Keep append + fsync before acknowledging (already done). Migrate or ignore the old ~/.local/state/tk/queue.jsonl (only test data; say which).
4. SAFETY: tk walks parent dirs to find .tickets/ (storage.rs find_tickets_dir). Creating ~/.tickets would make every directory under $HOME without its own .tickets use ~/.tickets as its ticket store. Make find_tickets_dir skip $HOME/.tickets (the global queue dir), with a test.
5. Reads vs writes: keep the single-process server (reads never see half writes). File watching / notify-on-write: DEFERRED (note it). Growth: drain/compaction is a separate ticket (below), not now.
6. Setup in this repo: there is no setup dir today (root has test.sh and generate-types.sh). Use `setup/`: setup/install.sh (cargo build --release, then atomic install to ~/.local/bin/tk: copy to a temp file in the same dir, then mv), setup/queue.sh (install/uninstall/status of macOS launchd user agents), setup/launchd/*.plist (tk queue serve; ssh -N -R 127.0.0.1:7878:127.0.0.1:7878 vert with keep-alive). Logs to ~/Library/Logs/tk/.

