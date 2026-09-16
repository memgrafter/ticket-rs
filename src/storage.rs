//! File-based ticket storage.
//!
//! Tickets are stored as markdown files with YAML frontmatter in `.tickets/`.
//! The storage layer handles:
//! - Finding the `.tickets/` directory by walking parent directories
//! - Parsing/serializing ticket files
//! - CRUD operations
//! - Partial ID resolution via an in-memory index

use crate::id::{ticket_filename, ticket_id_from_filename};
use crate::types::{CreateOptions, Data, Metadata, Note, Priority};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// File-based ticket storage.
pub struct Storage {
    /// Path to the .tickets directory.
    dir: PathBuf,
    /// In-memory index: partial -> full ID, lazily populated.
    index: RefCell<Index>,
}

/// In-memory index of all ticket IDs.
#[derive(Default)]
struct Index {
    ids: Vec<String>,
    dirty: bool,
}

impl Storage {
    /// Find the `.tickets/` directory by walking parent directories,
    /// or use the `TICKETS_DIR` env var if set.
    ///
    /// For read commands, returns error if no directory found.
    /// For write commands, initializes `.tickets/` in current dir.
    pub fn new(is_write: bool) -> Result<Self> {
        if let Ok(env_dir) = std::env::var("TICKETS_DIR") {
            let p = PathBuf::from(&env_dir);
            if is_write {
                fs::create_dir_all(&p).context("Failed to create TICKETS_DIR")?;
            }
            return Ok(Storage {
                dir: p,
                index: RefCell::new(Index::default()),
            });
        }

        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        if let Some(dir) = find_tickets_dir(&cwd) {
            return Ok(Storage {
                dir,
                index: RefCell::new(Index::default()),
            });
        }

        if is_write {
            let p = cwd.join(".tickets");
            fs::create_dir_all(&p).context("Failed to create .tickets directory")?;
            Ok(Storage {
                dir: p,
                index: RefCell::new(Index::default()),
            })
        } else {
            bail!(
                "no .tickets directory found (searched parent directories)\n\
                 Run 'tk create' to initialize, or set TICKETS_DIR env var"
            )
        }
    }

    /// Path to the .tickets directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Get the full path for a ticket file by ID.
    fn ticket_path(&self, id: &str) -> PathBuf {
        self.dir.join(ticket_filename(id))
    }

    /// Ensure the index is populated.
    fn ensure_index(&self) -> Result<()> {
        let mut idx = self.index.borrow_mut();
        if !idx.dirty {
            idx.ids.clear();
            let entries = fs::read_dir(&self.dir)
                .with_context(|| format!("Failed to read directory {:?}", self.dir))?;
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(id) = ticket_id_from_filename(&name) {
                    idx.ids.push(id);
                }
            }
            idx.ids.sort();
            idx.dirty = true;
        }
        Ok(())
    }

    /// Mark the index as dirty (after write operations).
    fn mark_dirty(&self) {
        self.index.borrow_mut().dirty = false;
    }

    /// Resolve a partial ID to a full ticket ID.
    pub fn resolve_id(&self, partial: &str) -> Result<String> {
        self.ensure_index()?;
        let idx = self.index.borrow();
        let ids: Vec<String> = idx.ids.clone();
        let result = crate::id::resolve_id(&ids, partial)?.clone();
        Ok(result.clone())
    }

    /// Read all ticket IDs (full).
    #[allow(dead_code)]
    pub fn all_ids(&self) -> Result<Vec<String>> {
        self.ensure_index()?;
        Ok(self.index.borrow().ids.clone())
    }

    /// Create a new ticket and return its ID.
    pub fn create(&self, opts: &CreateOptions) -> Result<String> {
        let dir_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();
        let id = crate::id::generate_id(&dir_name);

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let metadata = Metadata {
            id: id.clone(),
            status: "open".to_string(),
            open: true,
            deps: vec![],
            links: vec![],
            created: now.clone(),
            metadata_type: opts.create_type.clone().unwrap_or_else(|| "task".to_string()),
            priority: opts.priority.unwrap_or(Priority::P2),
            assignee: opts.assignee.clone().or_else(|| {
                // Default to git user.name
                std::process::Command::new("git")
                    .args(["config", "user.name"])
                    .output()
                    .ok()
                    .and_then(|o| {
                        if o.status.success() {
                            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                            if s.is_empty() { None } else { Some(s) }
                        } else {
                            None
                        }
                    })
            }),
            external_ref: opts.external_ref.clone(),
            parent: opts.parent.clone(),
            tags: opts.tags.clone(),
        };

        let data = Data {
            title: opts.title.clone().unwrap_or_else(|| "Untitled".into()),
            description: opts.description.clone(),
            design: opts.design.clone(),
            acceptance: opts.acceptance.clone(),
            notes: None,
        };

        let ticket = Ticket {
            metadata,
            data,
            non_standard_sections: Vec::new(),
        };
        let content = ticket.to_string();

        let path = self.ticket_path(&id);
        fs::write(&path, &content)
            .with_context(|| format!("Failed to write ticket file {:?}", path))?;

        self.mark_dirty();
        Ok(id)
    }

    /// Read a ticket by full ID.
    pub fn read(&self, id: &str) -> Result<Ticket> {
        let path = self.ticket_path(id);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read ticket file {:?}", path))?;
        Ticket::parse(&content, id)
    }

    /// Read a ticket by partial ID.
    #[allow(dead_code)]
    pub fn read_partial(&self, partial: &str) -> Result<Ticket> {
        let id = self.resolve_id(partial)?;
        self.read(&id)
    }

    /// Update a YAML field in the ticket's frontmatter.
    pub fn update_field(&self, id: &str, field: &str, value: &str) -> Result<()> {
        let path = self.ticket_path(id);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read ticket file {:?}", path))?;

        // Simple line-based YAML field replacement in frontmatter
        let mut in_frontmatter = false;
        let mut found = false;
        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let mut new_lines = Vec::new();

        for line in &lines {
            if line.trim() == "---" {
                in_frontmatter = !in_frontmatter;
                new_lines.push(line.clone());
                continue;
            }
            if in_frontmatter
                && (line.starts_with(&format!("{}:", field)) || line.starts_with(&format!("{}:", field)))
            {
                if !found {
                    new_lines.push(format!("{}: {}", field, value));
                    found = true;
                }
                continue;
            }
            new_lines.push(line.clone());
        }

        if !found {
            bail!("Field '{}' not found in ticket '{}'", field, id);
        }

        let new_content = new_lines.join("\n") + "\n";
        fs::write(&path, &new_content)
            .with_context(|| format!("Failed to write ticket file {:?}", path))?;

        Ok(())
    }

    /// Read all tickets.
    pub fn all(&self) -> Result<Vec<Ticket>> {
        self.ensure_index()?;
        let ids = self.index.borrow().ids.clone();
        let mut tickets = Vec::with_capacity(ids.len());
        for id in &ids {
            tickets.push(self.read(id)?);
        }
        Ok(tickets)
    }

    /// Read all tickets matching a filter.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<Ticket>> {
        let tickets = self.all()?;
        let mut results: Vec<Ticket> = tickets
            .into_iter()
            .filter(|t| query.matches(t))
            .collect();

        results.sort_by(|a, b| {
            a.metadata
                .priority
                .cmp(&b.metadata.priority)
                .then(a.metadata.id.cmp(&b.metadata.id))
        });

        Ok(results)
    }

    /// Add a note to a ticket.
    pub fn add_note(&self, id: &str, body: &str) -> Result<()> {
        let path = self.ticket_path(id);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read ticket file {:?}", path))?;

        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let note = format!("\n**{}**\n\n{}\n", timestamp, body);

        let new_content = if content.contains("## Notes") {
            // Append to existing Notes section
            content.trim_end().to_string() + "\n" + &note
        } else {
            content.trim_end().to_string() + "\n\n## Notes\n" + &note
        };

        fs::write(&path, new_content.as_bytes())
            .with_context(|| format!("Failed to write ticket file {:?}", path))?;

        Ok(())
    }

    /// Add an `open:` field to every ticket file that lacks one.
    ///
    /// The value is `false` when the ticket's `status` is `closed`, otherwise
    /// `true`. Files that already have an `open:` field are left untouched.
    /// Returns the `(id, open_value)` pairs for tickets that were modified,
    /// plus the count of tickets that already had the field (skipped).
    pub fn migrate(&self) -> Result<(Vec<(String, bool)>, usize)> {
        let entries = fs::read_dir(&self.dir)
            .with_context(|| format!("Failed to read directory {:?}", self.dir))?;
        let mut modified: Vec<(String, bool)> = Vec::new();
        let mut skipped = 0usize;

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".md") {
                continue;
            }
            let path = entry.path();
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read ticket file {:?}", path))?;

            match insert_open_field(&content) {
                Some((new_content, open_value)) => {
                    fs::write(&path, new_content.as_bytes())
                        .with_context(|| format!("Failed to write ticket file {:?}", path))?;
                    let id = ticket_id_from_filename(&name).unwrap_or_else(|| name.clone());
                    modified.push((id, open_value));
                }
                None => skipped += 1,
            }
        }

        modified.sort_by(|a, b| a.0.cmp(&b.0));
        Ok((modified, skipped))
    }
}

/// Search/filter query for tickets.
#[derive(Debug, Default)]
pub struct SearchQuery {
    pub open: Option<bool>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub ticket_type: Option<String>,
    pub tags: Option<Vec<String>>,
    pub search: Option<String>,
    #[allow(dead_code)]
    pub dedup: bool,
}

impl SearchQuery {
    fn matches(&self, ticket: &Ticket) -> bool {
        if let Some(open) = self.open {
            if ticket.metadata.open != open {
                return false;
            }
        }
        if let Some(ref status) = self.status {
            if ticket.metadata.status != *status {
                return false;
            }
        }
        if let Some(ref assignee) = self.assignee {
            if ticket.metadata.assignee.as_deref() != Some(assignee) {
                return false;
            }
        }
        if let Some(ref ticket_type) = self.ticket_type {
            if ticket.metadata.metadata_type != *ticket_type {
                return false;
            }
        }
        if let Some(ref tags) = self.tags {
            let ticket_tags = ticket.metadata.tags.as_deref().unwrap_or(&[]);
            // All specified tags must be present (intersection)
            for tag in tags {
                if !ticket_tags.contains(tag) {
                    return false;
                }
            }
        }
        if let Some(ref search) = self.search {
            let search_lower = search.to_lowercase();
            let body_lower = ticket.data.title.to_lowercase();
            if !body_lower.contains(&search_lower) {
                return false;
            }
        }
        true
    }
}

/// A parsed ticket file.
#[derive(Debug, Clone)]
pub struct Ticket {
    pub metadata: Metadata,
    pub data: Data,
    /// `##` section headings in the body that are not part of the known schema
    /// (description-before-first-heading / Design / Acceptance Criteria / Notes).
    /// Their content is parsed but not retained, so `tk show` cannot render it.
    /// Populated to warn the user instead of silently dropping the text.
    pub non_standard_sections: Vec<String>,
}

impl Ticket {
    /// Parse a ticket from its file content.
    pub fn parse(content: &str, default_id: &str) -> Result<Self> {
        let content = content.trim();

        // Split frontmatter and body
        let (yaml_str, body_str) = if let Some(rest) = content.strip_prefix("---") {
            if let Some(end_idx) = rest.find("---") {
                let yaml = rest[..end_idx].trim();
                let body = rest[end_idx + 3..].trim();
                (yaml, body)
            } else {
                bail!("Ticket file missing closing --- frontmatter delimiter");
            }
        } else {
            ("", content)
        };

        // Parse YAML frontmatter
        let metadata: Metadata = if yaml_str.is_empty() {
            return Err(anyhow::anyhow!("Missing YAML frontmatter"));
        } else {
            // Parse field by field to handle camelCase mapping
            parse_frontmatter(yaml_str, default_id)?
        };

        // Parse body
        let (data, non_standard_sections) = parse_body(body_str);

        Ok(Ticket {
            metadata,
            data,
            non_standard_sections,
        })
    }
}

impl std::fmt::Display for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::from("---\n");

        // Serialize metadata fields in canonical order
        out.push_str(&format!("id: {}\n", self.metadata.id));
        out.push_str(&format!("status: {}\n", self.metadata.status));
        out.push_str(&format!("open: {}\n", self.metadata.open));
        out.push_str(&format!("deps: {}\n", format_yaml_array(&self.metadata.deps)));
        out.push_str(&format!("links: {}\n", format_yaml_array(&self.metadata.links)));
        out.push_str(&format!("created: {}\n", self.metadata.created));
        out.push_str(&format!("type: {}\n", self.metadata.metadata_type));
        out.push_str(&format!("priority: {}\n", self.metadata.priority.to_u8()));
        if let Some(ref a) = self.metadata.assignee {
            out.push_str(&format!("assignee: {}\n", a));
        }
        if let Some(ref e) = self.metadata.external_ref {
            out.push_str(&format!("external-ref: {}\n", e));
        }
        if let Some(ref p) = self.metadata.parent {
            out.push_str(&format!("parent: {}\n", p));
        }
        if let Some(ref tags) = self.metadata.tags {
            if !tags.is_empty() {
                out.push_str(&format!("tags: {}\n", format_yaml_array(tags)));
            }
        }

        out.push_str("---\n");
        out.push_str(&format!("# {}\n", self.data.title));

        if let Some(ref desc) = self.data.description {
            out.push('\n');
            out.push_str(desc);
            out.push('\n');
        }
        if let Some(ref design) = self.data.design {
            out.push_str("\n## Design\n\n");
            out.push_str(design);
            out.push('\n');
        }
        if let Some(ref acceptance) = self.data.acceptance {
            out.push_str("\n## Acceptance Criteria\n\n");
            out.push_str(acceptance);
            out.push('\n');
        }
        if let Some(ref notes) = self.data.notes {
            if !notes.is_empty() {
                out.push_str("\n## Notes\n");
                for note in notes {
                    out.push_str(&format!("\n**{}**\n\n{}\n", note.timestamp, note.body));
                }
            }
        }

        write!(f, "{}", out)
    }
}

/// Parse YAML frontmatter string into Metadata.
fn parse_frontmatter(yaml_str: &str, default_id: &str) -> Result<Metadata> {
    let mut id = default_id.to_string();
    let mut status = String::new();
    let mut open: Option<bool> = None;
    let mut deps: Vec<String> = vec![];
    let mut links: Vec<String> = vec![];
    let mut created = String::new();
    let mut metadata_type = String::new();
    let mut priority = Priority::P2;
    let mut assignee: Option<String> = None;
    let mut external_ref: Option<String> = None;
    let mut parent: Option<String> = None;
    let mut tags: Option<Vec<String>> = None;

    for line in yaml_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "id" => id = value.to_string(),
                "status" => status = value.to_string(),
                "open" => open = Some(value == "true"),
                "deps" => {
                    deps = parse_yaml_array(value);
                }
                "links" => {
                    links = parse_yaml_array(value);
                }
                "created" => created = value.to_string(),
                "type" => metadata_type = value.to_string(),
                "priority" => {
                    priority = value.parse::<u8>().ok().and_then(Priority::from_u8).unwrap_or(Priority::P2);
                }
                "assignee" => assignee = Some(value.to_string()),
                "external-ref" => external_ref = Some(value.to_string()),
                "parent" => parent = Some(value.to_string()),
                "tags" => {
                    tags = Some(parse_yaml_array(value));
                }
                _ => {}
            }
        }
    }

    if created.is_empty() {
        created = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    }

    let open = match open {
        Some(open) => open,
        None => bail!(
            "ticket '{}' is missing the required 'open' field. Run `tk migrate` to add it, then retry.",
            id
        ),
    };

    Ok(Metadata {
        id,
        status,
        open,
        deps,
        links,
        created,
        metadata_type,
        priority,
        assignee,
        external_ref,
        parent,
        tags,
    })
}

/// Parse the body of a ticket (markdown after frontmatter).
fn parse_body(body_str: &str) -> (Data, Vec<String>) {
    let body = body_str.trim();

    // Extract title (first # heading)
    let title = body
        .lines()
        .find(|l| l.trim().starts_with("# "))
        .map(|l| l.trim().trim_start_matches("# ").to_string())
        .unwrap_or_else(|| "Untitled".to_string());

    // Split into sections
    let sections = split_sections(body);
    let description = sections.get("").cloned();
    let design = sections.get("Design").cloned();
    let acceptance = sections.get("Acceptance Criteria").cloned();
    let notes = sections.get("Notes").map(|n| parse_notes(n));

    // Known schema: empty key (description) + these four. Anything else is
    // parsed but discarded by this struct, so surface it for a display warning.
    const KNOWN: [&str; 4] = ["", "Design", "Acceptance Criteria", "Notes"];
    let non_standard: Vec<String> = sections
        .keys()
        .filter(|k| !KNOWN.contains(&k.as_str()))
        .map(|s| s.clone())
        .collect();

    (
        Data {
            title,
            description,
            design,
            acceptance,
            notes,
        },
        non_standard,
    )
}

/// Split body into sections by ## headings.
fn split_sections(body: &str) -> HashMap<String, String> {
    let mut sections: HashMap<String, String> = HashMap::new();
    let mut current_section = String::new();
    let mut current_content = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            // Save previous section
            if !current_content.is_empty() {
                sections.insert(
                    current_section.clone(),
                    current_content.join("\n").trim().to_string(),
                );
            }
            current_section = trimmed.trim_start_matches("## ").trim().to_string();
            current_content.clear();
        } else if trimmed.starts_with("# ") {
            // Top-level heading, skip (it's the title)
        } else {
            current_content.push(line);
        }
    }

    if !current_content.is_empty() {
        sections.insert(
            current_section.clone(),
            current_content.join("\n").trim().to_string(),
        );
    }

    sections
}

/// Parse notes from a Notes section body.
fn parse_notes(text: &str) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut current_timestamp = String::new();
    let mut current_body = Vec::new();
    let mut in_note = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("**") && trimmed.ends_with("**") {
            // Save previous note
            if !current_timestamp.is_empty() {
                notes.push(Note {
                    timestamp: current_timestamp.clone(),
                    body: current_body.join("\n").trim().to_string(),
                });
            }
            current_timestamp = trimmed.trim_matches('*').to_string();
            current_body.clear();
            in_note = true;
        } else if in_note {
            current_body.push(line);
        }
    }

    // Save last note
    if !current_timestamp.is_empty() {
        notes.push(Note {
            timestamp: current_timestamp.clone(),
            body: current_body.join("\n").trim().to_string(),
        });
    }

    notes
}

/// Parse a YAML array string like `[foo, bar]` into a Vec<String>.
fn parse_yaml_array(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() || s == "[]" {
        return vec![];
    }
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() {
        return vec![];
    }
    inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Format a Vec<String> as a YAML array string.
fn format_yaml_array(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", items.join(", "))
    }
}

/// If `content`'s frontmatter has no `open:` line, insert one (value `false`
/// when `status` is `closed`, else `true`) and return the new content plus the
/// value. Returns `None` when the file already has an `open:` line or has no
/// frontmatter block.
fn insert_open_field(content: &str) -> Option<(String, bool)> {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Locate the frontmatter block (between the first two `---` lines).
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            match start {
                None => start = Some(i),
                Some(_) => {
                    end = Some(i);
                    break;
                }
            }
        }
    }
    let (start, end) = match (start, end) {
        (Some(s), Some(e)) => (s, e),
        _ => return None,
    };

    let frontmatter = &lines[start + 1..end];

    // Already has an `open:` line? Leave it alone.
    if frontmatter.iter().any(|l| l.trim().starts_with("open:")) {
        return None;
    }

    // Derive the value from the existing `status:` line.
    let status_value = frontmatter
        .iter()
        .find(|l| l.trim().starts_with("status:"))
        .and_then(|l| l.trim().strip_prefix("status:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let open_value = status_value != "closed";

    // Insert right after the `status:` line (or after `id:` if there is no
    // status line), matching the canonical field order (id, status, open, …).
    let insert_at = frontmatter
        .iter()
        .position(|l| l.trim().starts_with("status:"))
        .map(|pos| start + 1 + pos + 1)
        .or_else(|| {
            frontmatter
                .iter()
                .position(|l| l.trim().starts_with("id:"))
                .map(|pos| start + 1 + pos + 1)
        })
        .unwrap_or(start + 1);

    lines.insert(insert_at, format!("open: {}", open_value));
    let mut new_content = lines.join("\n");
    if content.ends_with('\n') {
        new_content.push('\n');
    }
    Some((new_content, open_value))
}

/// Walk parent directories to find .tickets/.
fn find_tickets_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(ref d) = dir {
        if d.join(".tickets").is_dir() {
            return Some(d.join(".tickets"));
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_array() {
        assert_eq!(parse_yaml_array("[]"), Vec::<String>::new());
        assert_eq!(parse_yaml_array("[foo]"), vec!["foo"]);
        assert_eq!(parse_yaml_array("[foo, bar]"), vec!["foo", "bar"]);
        assert_eq!(parse_yaml_array("[foo, bar, baz]"), vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_format_yaml_array() {
        assert_eq!(format_yaml_array(&[]), "[]");
        assert_eq!(format_yaml_array(&["foo".into()]), "[foo]");
        assert_eq!(format_yaml_array(&["foo".into(), "bar".into()]), "[foo, bar]");
    }

    #[test]
    fn test_ticket_parse_and_serialize_roundtrip() {
        let content = "\
---
id: nw-5c46
status: open
open: true
deps: []
links: []
created: 2024-01-15T10:00:00Z
type: task
priority: 2
assignee: Alice
tags: [ui, urgent]
---

# My Ticket

This is the description.

## Design

Use a microservice architecture.

## Acceptance Criteria

Should pass all tests.

## Notes

**2024-01-15T12:00:00Z**

First note.
";

        let ticket = Ticket::parse(content, "nw-5c46").unwrap();
        assert_eq!(ticket.metadata.id, "nw-5c46");
        assert_eq!(ticket.metadata.status, "open");
        assert_eq!(ticket.metadata.metadata_type, "task");
        assert_eq!(ticket.metadata.priority, Priority::P2);
        assert_eq!(ticket.metadata.assignee.as_deref(), Some("Alice"));
        assert_eq!(ticket.metadata.tags.as_deref(), Some(&["ui".to_string(), "urgent".to_string()][..]));
        assert_eq!(ticket.data.title, "My Ticket");
        assert_eq!(ticket.data.description.as_deref(), Some("This is the description."));
        assert_eq!(ticket.data.design.as_deref(), Some("Use a microservice architecture."));
        assert_eq!(ticket.data.acceptance.as_deref(), Some("Should pass all tests."));
        assert!(ticket.data.notes.is_some());
        assert_eq!(ticket.data.notes.as_ref().unwrap().len(), 1);
        assert_eq!(ticket.data.notes.as_ref().unwrap()[0].timestamp, "2024-01-15T12:00:00Z");
        assert_eq!(ticket.data.notes.as_ref().unwrap()[0].body, "First note.");

        // Round-trip
        let serialized = format!("{}", ticket);
        let reparsed = Ticket::parse(&serialized, "nw-5c46").unwrap();
        assert_eq!(reparsed.metadata.id, ticket.metadata.id);
        assert_eq!(reparsed.metadata.status, ticket.metadata.status);
        assert_eq!(reparsed.data.title, ticket.data.title);
    }

    #[test]
    fn test_ticket_parse_minimal() {
        let content = "\
---
id: test-0001
status: open
open: true
deps: []
links: []
created: 2024-01-15T10:00:00Z
type: task
priority: 2
---

# Minimal
";
        let ticket = Ticket::parse(content, "test-0001").unwrap();
        assert_eq!(ticket.metadata.id, "test-0001");
        assert_eq!(ticket.data.title, "Minimal");
        assert!(ticket.data.description.is_none());
        assert!(ticket.metadata.assignee.is_none());
    }

    #[test]
    fn test_ticket_with_deps() {
        let content = "\
---
id: task-0001
status: open
open: true
deps: [task-0002]
links: []
created: 2024-01-15T10:00:00Z
type: task
priority: 2
---

# Main task
";
        let ticket = Ticket::parse(content, "task-0001").unwrap();
        assert_eq!(ticket.metadata.deps, vec!["task-0002"]);
    }

    #[test]
    fn test_parse_notes() {
        let notes_text = "\
**2024-01-15T12:00:00Z**

First note body.

**2024-01-16T12:00:00Z**

Second note body.
";
        let notes = parse_notes(notes_text);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].timestamp, "2024-01-15T12:00:00Z");
        assert_eq!(notes[0].body, "First note body.");
        assert_eq!(notes[1].timestamp, "2024-01-16T12:00:00Z");
        assert_eq!(notes[1].body, "Second note body.");
    }

    #[test]
    fn test_search_query_matches() {
        let ticket = Ticket::parse(
            "\
---
id: test-0001
status: open
open: true
deps: []
links: []
created: 2024-01-15T10:00:00Z
type: bug
priority: 1
assignee: Alice
tags: [ui, backend]
---

# Login fails
",
            "test-0001",
        )
        .unwrap();

        let q = SearchQuery {
            status: Some("open".to_string()),
            ..Default::default()
        };
        assert!(q.matches(&ticket));

        let q = SearchQuery {
            status: Some("closed".to_string()),
            ..Default::default()
        };
        assert!(!q.matches(&ticket));

        let q = SearchQuery {
            assignee: Some("Alice".into()),
            ..Default::default()
        };
        assert!(q.matches(&ticket));

        let q = SearchQuery {
            assignee: Some("Bob".into()),
            ..Default::default()
        };
        assert!(!q.matches(&ticket));

        let q = SearchQuery {
            ticket_type: Some("bug".to_string()),
            ..Default::default()
        };
        assert!(q.matches(&ticket));

        let q = SearchQuery {
            search: Some("login".into()),
            ..Default::default()
        };
        assert!(q.matches(&ticket));

        let q = SearchQuery {
            search: Some("nope".into()),
            ..Default::default()
        };
        assert!(!q.matches(&ticket));

        let q = SearchQuery {
            tags: Some(vec!["ui".into(), "backend".into()]),
            ..Default::default()
        };
        assert!(q.matches(&ticket));

        let q = SearchQuery {
            tags: Some(vec!["ui".into(), "missing".into()]),
            ..Default::default()
        };
        assert!(!q.matches(&ticket));
    }
}