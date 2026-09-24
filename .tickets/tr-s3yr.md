---
id: tr-s3yr
status: open
open: true
deps: []
links: []
created: 2026-09-24T13:53:51Z
type: bug
priority: 1
assignee: memgrafter
tags: [cli, rendering]
---
# tk show: only 4 section shapes render; all other headings hidden + warned

## Design

### Limitation (exact repro)
Given a ticket file containing sections `## Design`, `## References`, and
`## Key files` (the latter two are not in tk's render set), `tk show <id>`
prints:
(output indented; note the warning names the hidden sections by their heading text)

    warning: ticket <id> has non-standard section(s) not shown here:
    <the ##-headings that are not in the render set>
    tk only renders: (description before first ##), ## Design, ## Acceptance
    Criteria, ## Notes.

Only the description + the real `## Design` appear in the body; the other
`##` sections exist in the file but are hidden. (Sub-bug this also exposes:
tk's section parser is not code-fence-aware — heading-like lines inside fenced
blocks are parsed as sections, which is why this repro's own quoted output
would be mis-parsed if left at column 0.)

### Test cases
Ticket body shapes and what `tk show` must do (current → expected):

| # | body sections | today | expected after fix |
|---|---|---|---|
| 1 | desc + `## Design` + `## Acceptance Criteria` + `## Notes` (the 4 special) | all render, no warning | unchanged |
| 2 | desc + `## Design` + `## References` | `## References` hidden + warning | `## References` rendered (or correctly accounted for), no misleading warning |
| 3 | desc + `## Key files` only (no Design) | hidden + warning | rendered |
| 4 | desc only, no `##` sections | renders | unchanged |
| 5 | `### subsection` under a non-special `##` (e.g. `## Key files` > `### foo`) | hidden with parent | rendered with parent |
| 6 | duplicate `## Design` heading | untested | document + test behavior |

### Impact
- Research/architecture/design tickets naturally use headings outside the four;
  the only clean-render workaround is nesting everything under `## Design` as
  `###`, which is semantically wrong (`## References` is not design). Four
  tickets had to be formatted this way in one session.
- The warning reads as though the sections are broken; they are intact.
- `tk show` output is therefore not a faithful preview for most ticket shapes.

### Possible directions
- Render ALL `##` sections (keep the four special, stop treating others as
  warnings).
- Or expand the recognized set (e.g. `## Research`, `## Architecture`,
  `## References`) and warn only for unknown ones.
- Or make the warning informative: which sections render, which are hidden,
  and that the file is intact.

### Acceptance Criteria
- [ ] case #2/#3: non-special sections display in `tk show` (or are explicitly
      and correctly accounted for, no "broken" connotation)
- [ ] no "non-standard section" warning for common design-ticket headings
- [ ] case #5: subsections of rendered sections render too
