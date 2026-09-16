---
id: tr-yeuy
status: open
open: true
deps: []
links: [tr-zoaf]
created: 2026-07-20T02:02:43Z
type: feature
priority: 1
assignee: memgrafter
---
# Default mode: store tickets in ~/.tk by project ID with manifest mapping

Currently tk walks parent dirs to find .tickets/. Need a default mode where all tickets live in ~/.tk/<project-id>/ and a manifest maps project IDs to project directories. This keeps tickets out of repos by default and makes them centrally discoverable by file tree.
