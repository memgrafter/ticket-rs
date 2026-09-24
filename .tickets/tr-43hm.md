---
id: tr-43hm
status: open
open: true
deps: []
links: [tr-v181]
created: 2026-09-17T21:12:06Z
type: design
priority: 2
assignee: memgrafter
tags: [design, cli, ux]
---
# Design exploration: CRUD boundary between tk CLI and direct file edits

Design exploration: which ticket operations belong in the tk CLI, and which are better done by editing the file directly with an editor or an agent's edit tool.

Observation from day-to-day use with agents: tk shines at two things. Creating a ticket from the template (frontmatter, id allocation, sections) is high value and should stay in the CLI. Appending a timestamped note (tk add-note) is also better in the CLI because the timestamp and placement are mechanical. But editing existing fields (title, type, priority, tags, body sections) is a plain write op on a small markdown file. An agent already has a precise edit tool for that; wrapping every field in a tk subcommand duplicates it, and today there is no tk command for title, type, or priority at all, so the file gets edited anyway.

There is also a rendering constraint that interacts with this: tk show only renders the description plus Design, Acceptance Criteria, and Notes, and warns on any other heading. An author editing the body directly needs to know that, or the output degrades.

Goal: decide the boundary explicitly and document it, rather than growing the CLI toward full CRUD by accident.

## Design

Proposed boundary to explore:

- CLI owns operations that need computed or mechanical values: create (id, created timestamp, template), add-note (timestamp), status transitions (start, close, reopen), dep and link (both sides kept consistent).
- Direct file edit owns free-form content: title, description, Design and Acceptance Criteria bodies, type, priority, tags, assignee. These are one-line or one-block replacements with no derived state.

Questions:

1. Should tk gain small field setters anyway (tk set <id> priority 2) for shell-only use, or is that scope creep? If added, keep them to scalar frontmatter fields and never to body sections.
2. tk edit currently prints the path for EDITOR. Should it stay as the documented way in, plus a note in SKILL.md that agents should use their edit tool on that path?
3. Does the free-form section warning in tk show become a documented contract (only description, Design, Acceptance Criteria, Notes render), or should show render every section it finds?
4. Validation (tr-v181) is the safety net for direct edits. Does the boundary depend on it landing first?
5. Update SKILL.md with the decided boundary so agents stop routing every change through tk or, conversely, stop hand-editing timestamps and ids.

## Acceptance Criteria

- A written decision on which operations are CLI and which are file edits, recorded in this ticket and in SKILL.md.
- Any new tk subcommands that come out of it are filed as their own tickets linked here.
- tk show behaviour for non-standard sections is either documented or changed, per the decision.
