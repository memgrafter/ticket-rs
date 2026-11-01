---
id: tr-09qy
status: open
open: true
deps: []
links: []
created: 2026-11-01T16:19:13Z
type: feature
priority: 3
assignee: memgrafter
tags: [queue]
---
# tk queue: notify readers on write (file watch or push), deferred

Owner 2026-11-01: 'on write we could notify an active read, or read could watch the file, that there are new items. Or defer that.' Deferred. Related: the owner's separate socket/OS-wake listener design (not in this repo).
