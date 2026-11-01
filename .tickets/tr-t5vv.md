---
id: tr-t5vv
status: open
open: true
deps: []
links: []
created: 2026-11-01T16:19:13Z
type: feature
priority: 2
assignee: memgrafter
tags: [queue]
---
# tk queue: drain / compaction so the global queue file stays small

Owner 2026-11-01: 'the queue might to get pretty big, we will have to work on some batch drain mechanism or something.' Today the server replays the whole log at start and keeps every event in memory. Design a drain (e.g. snapshot + truncate, archive closed/consumed entries, keep the seq monotonic so poll cursors stay valid). Not started; design first.
