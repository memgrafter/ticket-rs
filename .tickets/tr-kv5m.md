---
id: tr-kv5m
status: open
open: true
deps: []
links: []
created: 2026-06-22T06:15:43Z
type: feature
priority: 1
assignee: trent
tags: [ux, cli]
---
# Add --description-file flag to tk create

tk create currently only accepts description text via the -d/--description CLI argument. For long descriptions with special characters (YAML, SQL, code snippets), passing them directly as CLI arguments fails because bash interprets $(), (), <>, quotes, etc. before reaching tk.

Example that fails:
```sh
tk create "Long description" -d "CREATE TABLE papers (arxiv_id TEXT PRIMARY KEY)"
```

Bash expands the parentheses and other special chars, so tk never sees the original text.

Workaround: write to file first, then use $(cat file):
```sh
printf '%s' 'CREATE TABLE papers (arxiv_id TEXT PRIMARY KEY)' > /tmp/desc.md
tk create "Long description" -d "$(cat /tmp/desc.md)"
```

But this is error-prone and requires manual temp file management.

Add a --description-file flag:
```sh
printf '%s' 'CREATE TABLE papers (arxiv_id TEXT PRIMARY KEY)' > /tmp/desc.md
tk create "Long description" -d /tmp/desc.md
```

This would be especially useful for epic tickets with full architecture definitions, API specs, etc. that contain lots of special characters.
