---
id: tr-alqa
status: open
open: true
deps: []
links: []
created: 2026-06-14T22:30:00Z
type: bug
priority: 1
assignee: memgrafter
---
# Fix array operations in jq-style queries

select(.deps | length > 0) returns ALL tickets regardless. Can't query 'tickets with dependencies' or 'tickets without dependencies'. Array comparisons like select(.deps != []) also broken.
