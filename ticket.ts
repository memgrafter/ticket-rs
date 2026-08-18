// Types for ticket's markdown + YAML frontmatter format.
//
// Id format: `<prefix>-<suffix>`
//   prefix: first letter of each hyphen/underscore segment of directory name (or first 3 chars)
//   suffix: 4-char lower-case alphanumeric random string, e.g. "nw-5c46", "tk-ab12"
//
// `open` is the single source of truth for open/closed:
//   open: true  = active (open or in progress)
//   open: false = terminal (closed / done / cancelled / ...)
// It is REQUIRED on every ticket. A ticket missing it is a parse error that
// points the user at `tk migrate`.
//
// `status` is a free-form label (e.g. "open", "in_progress", "done",
// "cancelled", or anything the user wants). The tool stores it verbatim and
// does not interpret it — the user assigns meaning in their own analytics.
//
// `type` is also a free-form label (conventional values: bug, feature, task,
// epic, chore — but any string is accepted).
//
// Priority: 0-4, 0=highest, default 2
// Assignee defaults to git user.name at creation
// External ref: e.g. "gh-123", "jira-456"
// Tags: comma-separated, stored as YAML array [tag1, tag2]
// Notes: appended via add-note, formatted as **<timestamp>**\n\n<body>
// Dependencies: asymmetric blocking relationship (A depends on B)
// Links: symmetric non-blocking relationship
// Parent: hierarchical grouping via parent field
//
// Files stored as .tickets/<id>.md.
// Frontmatter between `---` delimiters, YAML key-value pairs.
// Body after closing --- with # title, ## Design, ## Acceptance Criteria, ## Notes sections.

export type Priority = 0 | 1 | 2 | 3 | 4
export type Id = string & { readonly __brand: 'ticket-id' }

export interface Metadata {
  id: Id
  open: boolean
  status: string
  deps: Id[]
  links: Id[]
  created: string
  type: string
  priority: Priority
  assignee?: string
  externalRef?: string
  parent?: Id
  tags?: string[]
}

export interface Data {
  title: string
  description?: string
  design?: string
  acceptance?: string
  notes?: Note[]
}

export interface Note {
  timestamp: string
  body: string
}

export interface Frontmatter extends Metadata {}

export interface Tk {
  frontmatter: Frontmatter
  data: Data
  filename: string
}

export interface CreateOptions {
  title?: string
  description?: string
  design?: string
  acceptance?: string
  type?: string
  priority?: Priority
  assignee?: string
  externalRef?: string
  parent?: Id
  tags?: string[]
}

export interface Filter {
  open?: boolean
  status?: string
  assignee?: string
  type?: string
  tags?: string[]
}

export const FRONTMATTER_DELIMITER = '---' as const

export const CANONICAL_FIELD_ORDER: (keyof Metadata)[] = [
  'id',
  'status',
  'open',
  'deps',
  'links',
  'created',
  'type',
  'priority',
  'assignee',
  'externalRef',
  'parent',
  'tags',
] as const

export const TICKETS_DIR = '.tickets' as const
