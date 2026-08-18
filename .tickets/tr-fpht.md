---
id: tr-fpht
status: open
open: true
deps: []
links: []
created: 2026-07-14T03:56:09Z
type: bug
priority: 2
assignee: memgrafter
tags: [cli, ux]
---
# Backticks in --description are interpreted by shell before reaching tk


When using `tk create --description "..."` with markdown code blocks containing backticks, the shell interprets them as command substitution before tk receives the argument.

Example that fails:
```
tk create --description "Use \`_matching_staging_key()\` here"
```

The backticks cause bash to try to execute `_matching_staging_key()` as a command, corrupting the description.

Workaround: use single quotes for the outer string or escape backticks with backslash. But this is error-prone and not obvious to users.

Proposed fixes:
1. Read description from stdin when argument is `-`
2. Accept --description-file flag
3. Document the escaping requirement in help text

