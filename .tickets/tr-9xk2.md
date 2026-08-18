---
id: tr-9xk2
status: open
open: true
deps: []
links: []
created: 2026-06-14T22:35:00Z
type: bug
priority: 2
assignee: memgrafter
---
# Title rendering breaks when only in frontmatter, not in markdown heading

When creating tickets via the CLI, the title is stored in YAML frontmatter but `tk ls` shows "Untitled" if there's no corresponding `# Title` heading in the body. The frontmatter title field alone is not enough — the markdown heading is required for display.
