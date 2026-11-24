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
        let reserved = home_tickets_dir();
        if let Some(dir) = find_tickets_dir(&cwd, reserved.as_deref()) {
            return Ok(Storage {
                dir,
                index: RefCell::new(Index::default()),
            });
        }

        if is_write {
            let p = cwd.join(".tickets");
            if is_same_dir(&p, reserved.as_deref()) {
                bail!(
                    "refusing to keep tickets in {} (reserved for the tk queue)\n\
                     Run tk in a project directory, or set TICKETS_DIR",
                    p.display()
                );
            }
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
            body: String::new(),
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
        Ticket::parse(&self.read_raw(id)?, id)
    }

    /// Read a ticket file's content exactly as stored, by full ID.
    pub fn read_raw(&self, id: &str) -> Result<String> {
        let path = self.ticket_path(id);
        fs::read_to_string(&path).with_context(|| format!("Failed to read ticket file {:?}", path))
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

        // Find a real `## Notes` heading (not the text "## Notes" quoted in a
        // paragraph or code block) and put the note at the end of that section.
        let head_len = split_file(&content)?.map_or(0, |p| content.len() - p.body.len());
        let body = &content[head_len..];
        let notes = outline(body)
            .into_iter()
            .rev()
            .find(|s| s.level == 2 && s.heading == "Notes");
        let lines: Vec<&str> = body.lines().collect();
        let new_content = match notes {
            None => content.trim_end().to_string() + "\n\n## Notes\n" + &note,
            Some(s) if s.end == lines.len() => content.trim_end().to_string() + "\n" + &note,
            Some(s) => format!(
                "{}{}\n{}\n{}\n",
                &content[..head_len],
                lines[..s.end].join("\n").trim_end(),
                note,
                lines[s.end..].join("\n").trim_end()
            ),
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
    /// The markdown after the frontmatter exactly as stored. `data` holds only
    /// the known fields; this keeps every section for query and the queue.
    pub body: String,
}

/// A ticket file split at its frontmatter delimiters. All three are slices of
/// the original content, so callers can print any part verbatim.
pub struct FileParts<'a> {
    /// The frontmatter block, both `---` lines included.
    pub frontmatter: &'a str,
    /// The YAML between the `---` lines.
    pub yaml: &'a str,
    /// Everything after the closing `---` line.
    pub body: &'a str,
}

/// Split ticket file content into frontmatter and body. The delimiters are
/// whole `---` lines, so a `---` inside a YAML value does not end the block.
/// `Ok(None)` when the file has no opening `---`.
pub fn split_file(content: &str) -> Result<Option<FileParts<'_>>> {
    let start = content.len() - content.trim_start().len();
    let mut lines = content[start..].split_inclusive('\n');
    let first = match lines.next() {
        Some(l) if l.trim_end() == "---" => l,
        _ => return Ok(None),
    };
    let yaml_start = start + first.len();
    let mut pos = yaml_start;
    for line in lines {
        if line.trim_end() == "---" {
            let end = pos + line.len();
            return Ok(Some(FileParts {
                frontmatter: &content[start..end],
                yaml: &content[yaml_start..pos],
                body: &content[end..],
            }));
        }
        pos += line.len();
    }
    bail!("Ticket file missing closing --- frontmatter delimiter");
}

/// One markdown heading in a ticket body and the lines it spans.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// 1 for `#`, 2 for `##`, ... 6.
    pub level: usize,
    /// Heading text as written, without the `#` marks.
    pub heading: String,
    /// Index (into `body.lines()`) of the heading line.
    pub start: usize,
    /// Exclusive end: the next heading of the same or a higher level, or the
    /// line count. Subsections are inside the span.
    pub end: usize,
}

/// Parse an ATX heading line (`## Text`), allowing up to 3 spaces of indent
/// as markdown does. Returns (level, text).
fn heading_of(line: &str) -> Option<(usize, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let level = rest.len() - rest.trim_start_matches('#').len();
    if !(1..=6).contains(&level) {
        return None;
    }
    let text = &rest[level..];
    if !(text.is_empty() || text.starts_with(' ') || text.starts_with('\t')) {
        return None;
    }
    let text = text.trim();
    // Optional closing marks: `## Title ##`
    let stripped = text.trim_end_matches('#');
    let text = if stripped.len() < text.len() && (stripped.is_empty() || stripped.ends_with(' ')) {
        stripped.trim_end()
    } else {
        text
    };
    Some((level, text.to_string()))
}

/// For each line of `body`, its heading (level, text), or `None` for ordinary
/// lines. Lines inside ``` or ~~~ fenced code blocks are never headings.
fn heading_lines(body: &str) -> Vec<Option<(usize, String)>> {
    let mut fence: Option<(char, usize)> = None;
    body.lines()
        .map(|line| {
            let indent = line.len() - line.trim_start_matches(' ').len();
            let t = line.trim_start_matches(' ');
            let marker = t.chars().next().filter(|c| *c == '`' || *c == '~');
            let run = marker.map_or(0, |c| t.len() - t.trim_start_matches(c).len());
            match fence {
                Some((c, n)) => {
                    if indent <= 3 && marker == Some(c) && run >= n && t[run..].trim().is_empty() {
                        fence = None;
                    }
                    None
                }
                None if indent <= 3 && run >= 3 => {
                    fence = Some((marker.unwrap(), run));
                    None
                }
                None => heading_of(line),
            }
        })
        .collect()
}

/// All headings in a ticket body, in file order, each with its line span.
pub fn outline(body: &str) -> Vec<Section> {
    let heads: Vec<(usize, usize, String)> = heading_lines(body)
        .into_iter()
        .enumerate()
        .filter_map(|(i, h)| h.map(|(level, text)| (i, level, text)))
        .collect();
    let total = body.lines().count();
    heads
        .iter()
        .enumerate()
        .map(|(k, (start, level, heading))| Section {
            level: *level,
            heading: heading.clone(),
            start: *start,
            end: heads[k + 1..]
                .iter()
                .find(|(_, l, _)| l <= level)
                .map_or(total, |(i, _, _)| *i),
        })
        .collect()
}

impl Ticket {
    /// Parse a ticket from its file content.
    pub fn parse(content: &str, default_id: &str) -> Result<Self> {
        let (yaml_str, raw_body) = match split_file(content)? {
            Some(parts) => (parts.yaml.trim(), parts.body),
            None => ("", content),
        };
        let body_str = raw_body.trim();

        // Parse YAML frontmatter
        let metadata: Metadata = if yaml_str.is_empty() {
            return Err(anyhow::anyhow!("Missing YAML frontmatter"));
        } else {
            // Parse field by field to handle camelCase mapping
            parse_frontmatter(yaml_str, default_id)?
        };

        Ok(Ticket {
            metadata,
            data: parse_body(body_str),
            body: raw_body.to_string(),
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
fn parse_body(body_str: &str) -> Data {
    let body = body_str.trim();
    let heads = heading_lines(body);

    // Title: first `#` heading outside code fences
    let title = heads
        .iter()
        .flatten()
        .find(|(level, _)| *level == 1)
        .map(|(_, text)| text.clone())
        .unwrap_or_else(|| "Untitled".to_string());

    // Fill the struct's fields from the matching `##` sections. Display does
    // not use these (`tk show` prints the file); query, ls and the queue do.
    let sections = split_sections(body, &heads);
    let get = |name: &str| sections.get(name).cloned();
    Data {
        title,
        description: get(""),
        design: get("Design"),
        acceptance: get("Acceptance Criteria"),
        notes: get("Notes").map(|n| parse_notes(&n)),
    }
}

/// Split body into `##` sections keyed by heading text ("" = text before the
/// first `##`). `###` and deeper stay inside their section, `#` lines are
/// dropped, and duplicate headings are joined in file order.
fn split_sections(body: &str, heads: &[Option<(usize, String)>]) -> HashMap<String, String> {
    let mut sections: HashMap<String, String> = HashMap::new();
    let mut save = |name: &str, lines: &[&str]| {
        let text = lines.join("\n").trim().to_string();
        if text.is_empty() {
            return;
        }
        sections
            .entry(name.to_string())
            .and_modify(|s| {
                s.push_str("\n\n");
                s.push_str(&text);
            })
            .or_insert(text);
    };
    let mut current = String::new();
    let mut content: Vec<&str> = Vec::new();
    for (line, head) in body.lines().zip(heads) {
        match head {
            Some((2, text)) => {
                save(&current, &content);
                current = text.clone();
                content.clear();
            }
            Some((1, _)) => {}
            _ => content.push(line),
        }
    }
    save(&current, &content);
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
/// `$HOME/.tickets` holds the global tk queue file, not a project's tickets.
/// If parent-walking found it, every directory under `$HOME` without its own
/// `.tickets` would silently share it, so the walk skips it.
pub fn home_tickets_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".tickets"))
}

fn is_same_dir(a: &Path, b: Option<&Path>) -> bool {
    let Some(b) = b else { return false };
    if a == b {
        return true;
    }
    // Compare the parents: `a` may not exist yet (about to be created).
    let canon = |p: &Path| {
        let parent = p.parent().and_then(|d| fs::canonicalize(d).ok());
        (parent, p.file_name().map(|n| n.to_os_string()))
    };
    let (pa, na) = canon(a);
    pa.is_some() && (pa, na) == canon(b)
}

fn find_tickets_dir(start: &Path, skip: Option<&Path>) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(ref d) = dir {
        let candidate = d.join(".tickets");
        if candidate.is_dir() && !is_same_dir(&candidate, skip) {
            return Some(candidate);
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_tickets_dir_skips_reserved_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let project = home.join("code/project");
        fs::create_dir_all(home.join(".tickets")).unwrap();
        fs::create_dir_all(&project).unwrap();
        let reserved = home.join(".tickets");

        assert_eq!(find_tickets_dir(&project, Some(&reserved)), None);
        assert_eq!(find_tickets_dir(&project, None), Some(reserved.clone()));

        fs::create_dir_all(project.join(".tickets")).unwrap();
        assert_eq!(
            find_tickets_dir(&project, Some(&reserved)),
            Some(project.join(".tickets"))
        );
        // A .tickets above $HOME is still found.
        fs::create_dir_all(tmp.path().join(".tickets")).unwrap();
        assert_eq!(
            find_tickets_dir(&home.join("code"), Some(&reserved)),
            Some(tmp.path().join(".tickets"))
        );
    }

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

    #[test]
    fn test_split_file_whole_line_delimiters() {
        let raw = "---\nid: a-1\nexternal-ref: x---y\n---\n# T\n\nbody\n";
        let p = split_file(raw).unwrap().unwrap();
        assert_eq!(p.frontmatter, "---\nid: a-1\nexternal-ref: x---y\n---\n");
        assert_eq!(p.yaml, "id: a-1\nexternal-ref: x---y\n");
        assert_eq!(p.body, "# T\n\nbody\n");
        assert!(split_file("# no frontmatter").unwrap().is_none());
        assert!(split_file("---\nid: a\n").is_err());
    }

    #[test]
    fn test_outline_levels_spans_and_fences() {
        let body = "# T\n## A\na\n### A1\nx\n```\n## not a heading\n```\n## B\n    ## indented code\nb\n## B ##\n";
        let o = outline(body);
        let got: Vec<(usize, &str, usize, usize)> =
            o.iter().map(|s| (s.level, s.heading.as_str(), s.start, s.end)).collect();
        assert_eq!(
            got,
            vec![
                (1, "T", 0, 12),
                (2, "A", 1, 8),
                (3, "A1", 3, 8),
                (2, "B", 8, 11),
                (2, "B", 11, 12),
            ]
        );
    }

    #[test]
    fn test_parse_body_duplicates_fences_and_order() {
        let body = "# Title\n\ndesc\n\n## Design\none\n```\n## Notes\n```\n## Extra\nkept out of design\n## Design\ntwo\n";
        let d = parse_body(body);
        assert_eq!(d.title, "Title");
        assert_eq!(d.description.as_deref(), Some("desc"));
        assert_eq!(d.design.as_deref(), Some("one\n```\n## Notes\n```\n\ntwo"));
        assert!(d.notes.is_none());
    }
}
