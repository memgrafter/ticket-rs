// Types for ticket's markdown + YAML frontmatter format.
//
// Id format: `<prefix>-<suffix>`
//   prefix: first letter of each hyphen/underscore segment of directory name (or first 3 chars)
//   suffix: 4-char lower-case alphanumeric random string, e.g. "nw-5c46", "tk-ab12"
//
// Status lifecycle: open → in_progress → closed (reopen goes back to open)
// Type: bug, feature, task, epic, chore
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

export type Status = 'open' | 'in_progress' | 'closed'
export type Type = 'bug' | 'feature' | 'task' | 'epic' | 'chore'
export type Priority = 0 | 1 | 2 | 3 | 4
export type Id = string & { readonly __brand: 'ticket-id' }

export interface Metadata {
  id: Id
  status: Status
  deps: Id[]
  links: Id[]
  created: string
  type: Type
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
  type?: Type
  priority?: Priority
  assignee?: string
  externalRef?: string
  parent?: Id
  tags?: string[]
}

export interface Filter {
  status?: Status
  assignee?: string
  type?: Type
  tags?: string[]
}

export const FRONTMATTER_DELIMITER = '---' as const

export const CANONICAL_FIELD_ORDER: (keyof Metadata)[] = [
  'id',
  'status',
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
