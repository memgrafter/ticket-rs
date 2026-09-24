---
id: tr-r24v
status: open
open: true
deps: []
links: []
created: 2026-09-24T13:53:51Z
type: bug
priority: 1
assignee: memgrafter
tags: [cli, arg-parsing]
---
# tk create: description containing a '---' line fails with bare usage, no error

## Design

### Symptom (exact repro)
```sh
# desc.md:
#   ---
#   id: x
#   status: open
#   ---
#   Body line one.
tk create "Title" -t feature -p 2 -d "$(cat desc.md)"
# actual output (no 'error:' line, no diagnosis):
#   Usage: tk create --issue-type <ISSUE_TYPE> --priority <PRIORITY> --tags <TAGS> <TITLE>
#   For more information, try '--help'.
```
The ticket is NOT created. Confirmed 2026-09-24 creating a research ticket
whose body began with a YAML frontmatter block.

### Root cause hypothesis
clap treats `--` as the option-terminator sequence; a `-d` *value* containing
`---` corrupts argument parsing. Verify in the arg-parsing layer of `tk create`.

### Test cases (use these for unit/CLI coverage)
Each case is (description content) → (expected). Current behavior is noted;
the fix must make every case either create successfully or fail with an
**explicit `error: ...` line** — never a bare usage string as the only output.

| # | description contains | expected after fix | today |
|---|---|---|---|
| 1 | a full YAML frontmatter block (`---` line 1 and line 4, key: value between) | created, body intact | bare usage, nothing created |
| 2 | a single `---` horizontal-rule line mid-text (`text before\n---\ntext after`) | created, or explicit error naming line N | bare usage |
| 3 | `----` (four dashes) | same policy as #2 — document which | untested — add |
| 4 | `--` alone on a line | same policy as #2 | untested — add |
| 5 | no dashes at all (control) | created | created ✓ |
| 6 | backticks in the text (no dashes) | created | created ✓ (but see tr-fpht: inline -d without command-substitution quoting) |
| 7 | `---` as the ONLY content | explicit error (empty-after-strip) | untested — add |

### Acceptance Criteria
- [ ] cases #1-#2: create succeeds with the body intact, OR an explicit error
      names the offending line: `error: --description contains "--" (line N)`
- [ ] no code path prints the usage string as the only output of a failed create
- [ ] cases #3, #4, #7 added as tests with the chosen policy documented
- [ ] control cases #5, #6 still pass

### References
- tr-kv5m — `--description-file` flag (mitigates metachar fragility generally)
- tr-fpht — backticks in --description interpreted by shell
