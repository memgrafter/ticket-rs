---
id: tr-hslg
status: closed
open: false
deps: []
links: [tr-s3yr]
created: 2026-11-24T21:07:48Z
type: bug
priority: 2
assignee: memgrafter
tags: [cli, queue]
---
# tk query and queue mirror drop free-form sections and hand-written Notes text

Same data loss tk show had before tr-s3yr, in the two JSON outputs: `tk query` (src/display.rs ticket_to_json) and the queue mirror payload (src/queue.rs, the json! built for push_ticket). `tk query` sends only `description` from the body (no design, acceptance or notes at all). The queue mirror sends description, design, acceptance and parsed notes. So in query every ## section is missing, and in the mirror every other ## section plus any text in ## Notes before the first **timestamp** line never leaves the machine.

Proposed fix: add a `body` field holding the markdown after the frontmatter exactly as stored (storage::split_file gives it). Keep the existing fields for compatibility.

Separate from the show fix because it changes the queue's wire format: the server, the poll JSONL and any consumer of tk query see a new field.

## Acceptance Criteria

- tk query output has body equal to the file's body bytes
- queue mirror entries carry body; tests in tests/queue.rs cover it
- existing fields unchanged

## Notes

**2026-11-24T21:14:36Z**

Fixed: Ticket now keeps the stored body (storage::Ticket.body, the markdown after the frontmatter, byte for byte). tk query adds "body" to each JSONL line; the queue mirror snapshot adds "body" to the ticket object. Existing fields unchanged. query's JSON escaping now goes through serde_json, so control characters other than newline/tab/CR no longer produce invalid JSON. Checked on this repo's 27 tickets: every query line parses and body equals the stored body. Each mirror entry now roughly doubles in size (body repeats the parsed fields); largest ticket here is 12.5 KB against the server's 1 MiB request cap. Log growth is tr-t5vv's concern.
