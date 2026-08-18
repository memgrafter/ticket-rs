---
id: tr-nj30
status: open
open: true
deps: []
links: []
created: 2026-06-14T22:30:00Z
type: bug
priority: 2
assignee: memgrafter
---
# Fix complex multi-condition queries

Array comparisons don't work properly in jq-style filters. select(.deps != []) returns all tickets.
