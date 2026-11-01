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

## Notes

**2026-11-01T16:29:47Z**

Built and installed (agent, 2026-11-01). Commits on main, not pushed:
c1c35e3 fix: parent walk skips $HOME/.tickets, the queue dir
05e188a test: keep integration tests off a live TK_QUEUE_URL
b9ddd5d feat: named queues and raise groups; log in ~/.tickets
c27391c feat: setup scripts for install and queue launchd agents
36445a7 docs: queue names, raise groups, log path, setup

1. Named queues: entries and raises carry a free-form `queue` (default "default"). `--queue` on raise/ls/poll; HTTP param `queue`; pushed tickets keep their queue unless TK_QUEUE_NAME is set; a ticket made from a raise joins the raise's queue. Server indexes: queue -> entry keys and queue -> event positions, so a filtered read touches only that queue's items (hash lookup + k results; events by binary search on seq). poll --queue keeps its own cursor (cursor-<repo>--<queue>).
2. Raise groups (Sentry model): `--group G` / `group=G`. The first raise of (queue, repo, group) makes the entry; later raises update that entry: count, first_seen, last_seen, latest {title, description, host, at}. Server index (queue, repo, group) -> raise id, rebuilt from the log at start (tested). The poller that made the ticket adds ONE note per poll for new occurrences ("Raised again N time(s), C in all (queue:<id> occurrence C)", latest host/time/title, first 500 chars of the latest description); the marker makes it idempotent, and POST /noted records how many the ticket shows. No group = one ticket per raise (as before).
   Closed group raises again -> the ticket is REOPENED (open: true, status: open) with a "Regression:" note. Why: closed means fixed, so a new occurrence is evidence that it is not (Sentry treats a resolved issue that recurs as a regression); a flag on a closed ticket would be invisible to agents, because `tk ready` and `ls --open` show only open tickets. Cost: a group that is closed as "won't fix" reopens on its next raise; there is no mute/ignore state yet (open decision below). Note: tk stores no close time, so an occurrence that happened before the close but is polled after it also reopens.
3. Storage: default log ~/.tickets/_global_queue.jsond (NDJSON), append + fsync before the reply (unchanged). Old ~/.local/state/tk/queue.jsonl: never existed (all tests used --data in temp dirs), nothing to migrate. Lines without `queue` replay as "default".
4. Safety: find_tickets_dir skips $HOME/.tickets; `tk create` in $HOME is refused (it would make ~/.tickets the store again); TICKETS_DIR still overrides. Unit + integration test. Committed and installed before anything created ~/.tickets. Checked live: `tk ls` in ~ and ~/Documents -> "no .tickets directory found"; `tk create` in ~ -> refused. Only one tk on PATH (~/.local/bin/tk).
5. Reads/writes: single-process server unchanged. File watching / notify-on-write: deferred (tr-09qy). Growth: drain/compaction deferred (tr-t5vv). Today the poller fetches all raise entries of its repo each poll; that grows with the log until drain exists.
6. Setup: setup/install.sh (release build; copy to a temp file in ~/.local/bin, run --version on it, then mv over tk), setup/queue.sh install|uninstall|status, setup/launchd/local.tk.queue-serve.plist and local.tk.queue-tunnel-vert.plist (`ssh -N -T -o BatchMode=yes -o ConnectTimeout=15 -o ExitOnForwardFailure=yes -o ServerAliveInterval=30 -o ServerAliveCountMax=3 -R 127.0.0.1:7878:127.0.0.1:7878 vert`; KeepAlive; ThrottleInterval 30). Logs ~/Library/Logs/tk/.

Tests (./test.sh --all, run with TK_QUEUE_URL=http://127.0.0.1:7878 exported to prove tests don't touch the live queue; live log stayed 0 bytes):
  unit 45 passed; integration 78 passed (77 existing unchanged + 1 new $HOME test); queue 14 passed (new: named queues, raise groups incl. regression reopen and per-queue groups, default log path; restart test now also checks the group index).
  Clippy step still fails only on the old lint in src/storage.rs (explicit clone closure, now line 604), not from this work.

Installed and loaded (owner-approved):
- ~/.local/bin/tk replaced atomically by setup/install.sh. Old binary kept at ~/backups/tk-before-tr-ev9a-20261101 (restore: cp it to a temp file in ~/.local/bin, then mv over tk). Checked: `tk --version` -> tk 0.1.0; `tk queue --help` works; `tk ls` in ~/code/characteragent.ai-social-content lists its tickets.
- launchd agents loaded (gui domain): local.tk.queue-serve (running, ~/.local/bin/tk queue serve --port 7878, log ~/.tickets/_global_queue.jsond) and local.tk.queue-tunnel-vert (running). `setup/queue.sh status`: health OK on the Mac and from vert through the tunnel ({"ok":true,"seq":0}). Killed the tunnel ssh once: launchd restarted it in about 15 s, health from vert OK again. No test data was written to the live queue.
- The tunnel depends on the vert entry in ~/.ssh/config, which is marked TEMPORARY (WAN outage: Hostname 192.168.1.22, BindAddress 192.168.1.105). If the Mac's LAN address changes or that block is reverted, the tunnel ssh fails and launchd retries every 30 s; check ~/Library/Logs/tk/queue-tunnel-vert.log. Key ~/.ssh/id_ed25519 has no passphrase, so no agent is needed under launchd.

Profiles (backup first, one guarded line, skipped if TK_QUEUE_URL already present):
- Mac: ~/.zshrc (the Mac's shell is zsh; the owner said .bashrc). ~/.zshrc is a symlink to ~/.dotfiles/.zshrc, so the change is in that dotfiles repo (not committed). Backup ~/.zshrc.bak-tk-20261101. Diff: `24a25 > export TK_QUEUE_URL=http://127.0.0.1:7878  # tk queue (tr-ev9a)`. `zsh -ic` sees it. Every new interactive Mac shell now sends ticket changes to the queue.
- vert: ~/.bashrc -> ~/.dotfiles/.bashrc (robbintt/dotfiles, already had other uncommitted changes; not committed). Backup ~/.bashrc.bak-tk-20261101. Diff: `236a237 > export TK_QUEUE_URL=...`. `bash -ic` sees it and curl $TK_QUEUE_URL/health works. The line is after the non-interactive `return` (lines 4-6), so cron/nohup batch jobs do NOT get it: the TRELLIS script should set TK_QUEUE_URL itself (or default to http://127.0.0.1:7878).
- argent: NOT changed. Reached it via `ssh vert ssh argent` (works). Its ~/.bashrc is a symlink into ~/code/autocatalytic-infra/ansible/roles/bashrc/files/bashrc, a repo-managed file of an Ansible role used by playbook.yml, playbook-argent.yml and playbook-vert.yml; editing it changes fleet config on the next playbook run. argent also has no tunnel yet (nothing on its 127.0.0.1:7878), so the variable would point at nothing.

Open decisions for the owner
1. argent profile: (a) add the line to the Ansible role file (all hosts using the role get it on the next run), (b) add it to argent's ~/.bash_aliases (plain local file, sourced by its .bashrc; argent only), or (c) wait until argent has a tunnel. Recommendation: (b) when the tunnel exists.
2. argent tunnel (no services created): (a) a systemd user unit on vert: `ssh -N -o ExitOnForwardFailure=yes -o ServerAliveInterval=30 -R 127.0.0.1:7878:127.0.0.1:7878 argent`, which chains argent's port to vert's port, which is the Mac tunnel (vert already has a key for argent); (b) a second launchd agent on the Mac with `-J vert ... argent` (needs the Mac's key in archimedes@argent authorized_keys; today it is refused); (c) argent runs `ssh -N -L 7878:127.0.0.1:7878 vert` (needs argent -> vert ssh). Recommendation: (a), one hop, keys already exist; but both hops must be up.
3. Mute/ignore for groups (so a "won't fix" group stops reopening), e.g. a tag on the ticket that the poller respects.
4. Commit the profile lines in ~/.dotfiles on the Mac and vert (left uncommitted; both repos had other uncommitted changes).
5. The vert ssh config block is temporary; when it is reverted, re-run `setup/queue.sh status`.
