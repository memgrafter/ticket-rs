//! ticket-rs: A ticket system with dependency tracking.
//!
//! Tickets are markdown files with YAML frontmatter in `.tickets/`.
//! See ticket.ts for the canonical type definitions.

mod display;
mod graph;
mod id;
mod storage;
pub mod types;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use display::{format_ticket_blocked, format_ticket_list, format_ticket_show, format_tickets_jsonl};
use graph::{render_dep_tree, DependencyGraph};
use storage::{SearchQuery, Storage, Ticket};
use std::collections::HashMap;
use types::Priority;

/// Minimal ticket system with dependency tracking.
#[derive(Parser)]
#[command(name = "tk", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a ticket, prints ID
    Create {
        /// Title (optional, defaults to "Untitled")
        title: Option<String>,
        /// Description text
        #[arg(short = 'd', long)]
        description: Option<String>,
        /// Design notes
        #[arg(long)]
        design: Option<String>,
        /// Acceptance criteria
        #[arg(long)]
        acceptance: Option<String>,
        /// Type (free-form, e.g. bug, feature, task, epic, chore)
        #[arg(short = 't', long)]
        issue_type: Option<String>,
        /// Priority 0-4, 0=highest
        #[arg(short = 'p', long)]
        priority: Option<u8>,
        /// Assignee
        #[arg(short = 'a', long)]
        assignee: Option<String>,
        /// External reference (e.g., gh-123, JIRA-456)
        #[arg(long)]
        external_ref: Option<String>,
        /// Parent ticket ID
        #[arg(long)]
        parent: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
    },
    /// Mark the ticket open and in progress
    Start {
        id: String,
    },
    /// Mark the ticket closed (terminal)
    Close {
        id: String,
    },
    /// Reopen a closed ticket
    Reopen {
        id: String,
    },
    /// Add an `open:` field to every ticket that lacks one (no arguments)
    Migrate,
    /// Set the free-form status label (any string)
    Status {
        id: String,
        status: String,
    },
    /// Add dependency (id depends on dep-id)
    Dep {
        id: String,
        dep_id: String,
    },
    /// Remove dependency
    Undep {
        id: String,
        dep_id: String,
    },
    /// Link tickets together (symmetric)
    Link {
        ids: Vec<String>,
    },
    /// Remove link between tickets
    Unlink {
        id: String,
        target_id: String,
    },
    /// List tickets with optional filters
    #[command(alias = "list")]
    Ls {
        /// Filter by the free-form status string
        #[arg(long)]
        status: Option<String>,
        /// Only show open (active) tickets
        #[arg(long)]
        open: bool,
        /// Only show closed (terminal) tickets
        #[arg(long)]
        closed: bool,
        /// Filter by assignee
        #[arg(short = 'a')]
        assignee: Option<String>,
        /// Filter by the free-form type string
        #[arg(short = 'T')]
        ticket_type: Option<String>,
        /// Filter by tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
        /// Search in title
        #[arg(short = 's', long)]
        search: Option<String>,
    },
    /// List open/in-progress tickets with deps resolved
    Ready {
        #[arg(short = 'a')]
        assignee: Option<String>,
        #[arg(short = 'T')]
        tag: Option<String>,
    },
    /// List open/in-progress tickets with unresolved deps
    Blocked {
        #[arg(short = 'a')]
        assignee: Option<String>,
        #[arg(short = 'T')]
        tag: Option<String>,
    },
    /// List recently closed tickets
    Closed {
        /// Number of tickets to show (default 20)
        #[arg(long, default_value = "20")]
        limit: usize,
        #[arg(short = 'a')]
        assignee: Option<String>,
        #[arg(short = 'T')]
        tag: Option<String>,
    },
    /// Display a ticket
    Show {
        id: String,
    },
    /// Edit a ticket file (prints path for $EDITOR)
    Edit {
        id: String,
    },
    /// Append timestamped note
    AddNote {
        id: String,
        /// Note text (or pipe via stdin)
        text: Option<String>,
    },
    /// Output tickets as JSONL, optionally filtered with jq-style expression
    Query {
        /// jq-style filter expression (e.g., '.status == "open"')
        filter: Option<String>,
    },
    /// Show dependency tree
    DepTree {
        id: String,
        /// Disable deduplication
        #[arg(long)]
        full: bool,
    },
    /// Find dependency cycles
    DepCycle,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Create {
            title,
            description,
            design,
            acceptance,
            issue_type,
            priority,
            assignee,
            external_ref,
            parent,
            tags,
        } => cmd_create(
            title,
            description,
            design,
            acceptance,
            issue_type,
            priority,
            assignee,
            external_ref,
            parent,
            tags,
        ),
        Command::Start { id } => cmd_start(&id),
        Command::Close { id } => cmd_close(&id),
        Command::Reopen { id } => cmd_reopen(&id),
        Command::Status { id, status } => cmd_status(&id, &status),
        Command::Dep { id, dep_id } => cmd_dep_add(&id, &dep_id),
        Command::Undep { id, dep_id } => cmd_dep_remove(&id, &dep_id),
        Command::Link { ids } => cmd_link(&ids),
        Command::Unlink { id, target_id } => cmd_unlink(&id, &target_id),
        Command::Ls {
            status,
            open,
            closed,
            assignee,
            ticket_type,
            tags,
            search,
        } => cmd_list(status, open, closed, assignee, ticket_type, tags, search),
        Command::Ready { assignee, tag } => cmd_ready(assignee, tag),
        Command::Blocked { assignee, tag } => cmd_blocked(assignee, tag),
        Command::Closed {
            limit,
            assignee,
            tag,
        } => cmd_closed(limit, assignee, tag),
        Command::Show { id } => cmd_show(&id),
        Command::Edit { id } => cmd_edit(&id),
        Command::AddNote { id, text } => cmd_add_note(&id, text),
        Command::Query { filter } => cmd_query(filter),
        Command::DepTree { id, full } => cmd_dep_tree(&id, full),
        Command::Migrate => cmd_migrate(),
        Command::DepCycle => cmd_dep_cycle(),
    }
}

fn make_storage(is_write: bool) -> Result<Storage> {
    Storage::new(is_write)
}

#[allow(clippy::too_many_arguments)]
fn cmd_create(
    title: Option<String>,
    description: Option<String>,
    design: Option<String>,
    acceptance: Option<String>,
    issue_type: Option<String>,
    priority: Option<u8>,
    assignee: Option<String>,
    external_ref: Option<String>,
    parent: Option<String>,
    tags: Option<String>,
) -> Result<()> {
    let storage = make_storage(true)?;

    // Resolve parent if specified
    let resolved_parent = if let Some(ref p) = parent {
        Some(storage.resolve_id(p)?)
    } else {
        None
    };

    let parsed_tags = tags
        .as_ref()
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

    let parsed_priority = priority.and_then(Priority::from_u8);

    let opts = types::CreateOptions {
        title,
        description,
        design,
        acceptance,
        create_type: issue_type,
        priority: parsed_priority,
        assignee,
        external_ref,
        parent: resolved_parent,
        tags: parsed_tags,
    };

    let id = storage.create(&opts)?;
    println!("{}", id);
    Ok(())
}

fn cmd_status(id: &str, new_status: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved = storage.resolve_id(id)?;
    let _ = storage.read(&resolved)?;
    storage.update_field(&resolved, "status", new_status)?;
    println!("Updated {} status -> {}", resolved, new_status);
    Ok(())
}

fn cmd_start(id: &str) -> Result<()> {
    set_lifecycle(id, true, "in_progress")
}

fn cmd_close(id: &str) -> Result<()> {
    set_lifecycle(id, false, "closed")
}

fn cmd_reopen(id: &str) -> Result<()> {
    set_lifecycle(id, true, "open")
}

fn set_lifecycle(id: &str, open: bool, status: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved = storage.resolve_id(id)?;
    let _ = storage.read(&resolved)?;
    storage.update_field(&resolved, "open", if open { "true" } else { "false" })?;
    storage.update_field(&resolved, "status", status)?;
    println!("Updated {} -> open: {}, status: {}", resolved, open, status);
    Ok(())
}

fn cmd_migrate() -> Result<()> {
    let storage = match make_storage(false) {
        Ok(s) => s,
        Err(_) => {
            println!("No .tickets directory found — nothing to migrate.");
            return Ok(());
        }
    };
    let (modified, skipped) = storage.migrate()?;
    for (id, open_value) in &modified {
        println!("  {}: open: {}", id, open_value);
    }
    println!(
        "Migrated {} ticket(s); {} already had an `open` field.",
        modified.len(),
        skipped
    );
    Ok(())
}

fn cmd_dep_add(id: &str, dep_id: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved_id = storage.resolve_id(id)?;
    let resolved_dep = storage.resolve_id(dep_id)?;

    // Prevent circular deps at add-time (simple case: self-dep)
    if resolved_id == resolved_dep {
        bail!("cannot depend on itself");
    }

    let ticket = storage.read(&resolved_id)?;
    if ticket.metadata.deps.contains(&resolved_dep) {
        println!("Dependency already exists");
        return Ok(());
    }

    // Update deps in the file
    let mut new_deps = ticket.metadata.deps.clone();
    new_deps.push(resolved_dep.clone());
    let yaml = format_yaml_array(&new_deps);
    storage.update_field(&resolved_id, "deps", &yaml)?;

    println!("Added dependency: {} -> {}", resolved_id, resolved_dep);
    Ok(())
}

fn cmd_dep_remove(id: &str, dep_id: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved_id = storage.resolve_id(id)?;
    let resolved_dep = storage.resolve_id(dep_id)?;

    let ticket = storage.read(&resolved_id)?;
    if !ticket.metadata.deps.contains(&resolved_dep) {
        bail!("Dependency not found");
    }

    let new_deps: Vec<String> = ticket
        .metadata
        .deps
        .into_iter()
        .filter(|d| d != &resolved_dep)
        .collect();
    let yaml = format_yaml_array(&new_deps);
    storage.update_field(&resolved_id, "deps", &yaml)?;

    println!("Removed dependency: {} -/-> {}", resolved_id, resolved_dep);
    Ok(())
}

fn cmd_link(ids: &[String]) -> Result<()> {
    if ids.len() < 2 {
        bail!("Usage: tk link <id> <id> [id...]");
    }

    let storage = make_storage(false)?;
    let mut resolved_ids = Vec::new();
    for id in ids {
        resolved_ids.push(storage.resolve_id(id)?);
    }

    let mut total_links = 0;
    for (i, id) in resolved_ids.iter().enumerate() {
        let ticket = storage.read(id)?;
        let mut new_links = ticket.metadata.links.clone();

        for (j, other) in resolved_ids.iter().enumerate() {
            if i == j {
                continue;
            }
            if !new_links.contains(other) {
                new_links.push(other.clone());
                total_links += 1;
            }
        }

        if !new_links.eq(&ticket.metadata.links) {
            storage.update_field(id, "links", &format_yaml_array(&new_links))?;
        }
    }

    if total_links == 0 {
        println!("All links already exist");
    } else {
        println!(
            "Added {} link(s) between {} tickets",
            total_links,
            resolved_ids.len()
        );
    }
    Ok(())
}

fn cmd_unlink(id: &str, target_id: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved_id = storage.resolve_id(id)?;
    let resolved_target = storage.resolve_id(target_id)?;

    for rid in &[resolved_id.clone(), resolved_target.clone()] {
        let ticket = storage.read(rid)?;
        if !ticket.metadata.links.contains(&resolved_target)
            && !ticket.metadata.links.contains(&resolved_id)
        {
            continue;
        }
        let new_links: Vec<String> = ticket
            .metadata
            .links
            .into_iter()
            .filter(|l| l != &resolved_id && l != &resolved_target)
            .collect();
        storage.update_field(rid, "links", &format_yaml_array(&new_links))?;
    }

    println!("Removed link: {} <-> {}", resolved_id, resolved_target);
    Ok(())
}

fn cmd_list(
    status: Option<String>,
    open: bool,
    closed: bool,
    assignee: Option<String>,
    ticket_type: Option<String>,
    tags: Option<String>,
    search: Option<String>,
) -> Result<()> {
    let storage = make_storage(false)?;

    if open && closed {
        bail!("cannot filter by both --open and --closed");
    }
    let parsed_tags = tags
        .as_ref()
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

    let query = SearchQuery {
        open: if open { Some(true) } else if closed { Some(false) } else { None },
        status,
        assignee,
        ticket_type,
        tags: parsed_tags,
        search,
        ..Default::default()
    };

    let tickets = storage.search(&query)?;
    for ticket in &tickets {
        println!("{}", format_ticket_list(ticket, true));
    }
    Ok(())
}

fn cmd_ready(assignee: Option<String>, tag: Option<String>) -> Result<()> {
    let storage = make_storage(false)?;
    let tickets = storage.all()?;
    let open_map: HashMap<String, bool> = tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.metadata.open))
        .collect();
    let graph = DependencyGraph::build(&tickets);

    let mut ready: Vec<&Ticket> = tickets
        .iter()
        .filter(|t| {
            t.metadata.open
                && graph.is_ready(&t.metadata.id, &open_map)
                && assignee
                    .as_ref()
                    .map(|a| t.metadata.assignee.as_deref() == Some(a.as_str()))
                    .unwrap_or(true)
                && tag
                    .as_ref()
                    .map(|tag_val| {
                        t.metadata
                            .tags
                            .as_ref()
                            .map(|tags| tags.contains(tag_val))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
        })
        .collect();

    ready.sort_by(|a, b| {
        a.metadata
            .priority
            .cmp(&b.metadata.priority)
            .then(a.metadata.id.cmp(&b.metadata.id))
    });

    for ticket in &ready {
        println!("{}", format_ticket_list(ticket, false));
    }
    Ok(())
}

fn cmd_blocked(assignee: Option<String>, tag: Option<String>) -> Result<()> {
    let storage = make_storage(false)?;
    let tickets = storage.all()?;
    let open_map: HashMap<String, bool> = tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.metadata.open))
        .collect();
    let statuses: HashMap<String, String> = tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.metadata.status.clone()))
        .collect();
    let graph = DependencyGraph::build(&tickets);

    let mut blocked: Vec<&Ticket> = tickets
        .iter()
        .filter(|t| {
            t.metadata.open
                && graph.is_blocked(&t.metadata.id, &open_map)
                && assignee
                    .as_ref()
                    .map(|a| t.metadata.assignee.as_deref() == Some(a.as_str()))
                    .unwrap_or(true)
                && tag
                    .as_ref()
                    .map(|tag_val| {
                        t.metadata
                            .tags
                            .as_ref()
                            .map(|tags| tags.contains(tag_val))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
        })
        .collect();

    blocked.sort_by(|a, b| {
        a.metadata
            .priority
            .cmp(&b.metadata.priority)
            .then(a.metadata.id.cmp(&b.metadata.id))
    });

    for ticket in &blocked {
        let blockers = graph.blockers(&ticket.metadata.id, &open_map);
        println!("{}", format_ticket_blocked(ticket, &blockers, &statuses));
    }
    Ok(())
}

fn cmd_closed(limit: usize, assignee: Option<String>, tag: Option<String>) -> Result<()> {
    let storage = make_storage(false)?;
    let tickets = storage.all()?;

    let mut closed: Vec<&Ticket> = tickets
        .iter()
        .filter(|t| {
            !t.metadata.open
                && assignee
                    .as_ref()
                    .map(|a| t.metadata.assignee.as_deref() == Some(a.as_str()))
                    .unwrap_or(true)
                && tag
                    .as_ref()
                    .map(|tag_val| {
                        t.metadata
                            .tags
                            .as_ref()
                            .map(|tags| tags.contains(tag_val))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
        })
        .collect();

    // Sort by created date descending (most recent first)
    closed.sort_by(|a, b| b.metadata.created.cmp(&a.metadata.created));

    for ticket in closed.iter().take(limit) {
        println!("{}", format_ticket_list(ticket, false));
    }
    Ok(())
}

fn cmd_show(id: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved = storage.resolve_id(id)?;
    let ticket = storage.read(&resolved)?;
    let all_tickets = storage.all()?;

    let open_map: HashMap<String, bool> = all_tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.metadata.open))
        .collect();
    let statuses: HashMap<String, String> = all_tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.metadata.status.clone()))
        .collect();
    let titles: HashMap<String, String> = all_tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.data.title.clone()))
        .collect();
    let graph = DependencyGraph::build(&all_tickets);

    let output = format_ticket_show(&ticket, &open_map, &statuses, &titles, &graph, &all_tickets);
    println!("{}", output.trim_end());
    Ok(())
}

fn cmd_edit(id: &str) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved = storage.resolve_id(id)?;
    let path = storage.dir().join(format!("{}.md", resolved));

    println!("Edit ticket file: {}", path.display());
    Ok(())
}

fn cmd_add_note(id: &str, text: Option<String>) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved = storage.resolve_id(id)?;
    let _ = storage.read(&resolved)?;

    let body = match text {
        Some(t) => t,
        None => {
            // Read from stdin if not a TTY
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf.trim().to_string()
        }
    };

    storage.add_note(&resolved, &body)?;
    println!("Note added to {}", resolved);
    Ok(())
}

fn cmd_query(filter: Option<String>) -> Result<()> {
    let storage = make_storage(false)?;
    let tickets = storage.all()?;

    let filtered: Vec<Ticket> = if let Some(ref f) = filter {
        // Simple field-based filtering for jq-like expressions
        // Supports: .status == "open", .type == "bug", etc.
        tickets
            .into_iter()
            .filter(|t| matches_jq_filter(t, f))
            .collect()
    } else {
        tickets
    };

    let jsonl = format_tickets_jsonl(&filtered);
    if !jsonl.is_empty() {
        println!("{}", jsonl);
    }
    Ok(())
}

/// Simple jq-like filter matcher.
/// Supports: `.field == "value"` and `.field != "value"`
fn matches_jq_filter(ticket: &Ticket, filter: &str) -> bool {
    let filter = filter.trim();

    // Handle select() wrapper
    let inner = if filter.starts_with("select(") && filter.ends_with(')') {
        &filter[7..filter.len() - 1]
    } else {
        filter
    };

    let inner = inner.trim();

    // Parse: .field == "value" or .field != "value"
    let (negate, op) = if inner.contains("!=") {
        (true, "!=")
    } else if inner.contains("==") {
        (false, "==")
    } else {
        return true;
    };

    let parts: Vec<&str> = inner.splitn(2, op).collect();
    if parts.len() != 2 {
        return true;
    }

    let field = parts[0].trim().trim_start_matches('.');
    let value = parts[1].trim().trim_matches('"').trim();

    let ticket_value = match field {
        "status" => Some(ticket.metadata.status.clone()),
        "open" => Some(ticket.metadata.open.to_string()),
        "type" => Some(ticket.metadata.metadata_type.clone()),
        "priority" => Some(ticket.metadata.priority.to_u8().to_string()),
        "assignee" => ticket.metadata.assignee.clone(),
        _ => None,
    };

    match ticket_value {
        Some(ref v) if negate => v != value,
        Some(ref v) => v == value,
        None => true,
    }
}

fn cmd_dep_tree(id: &str, full: bool) -> Result<()> {
    let storage = make_storage(false)?;
    let resolved = storage.resolve_id(id)?;
    let tickets = storage.all()?;

    let statuses: HashMap<String, String> = tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.metadata.status.clone()))
        .collect();
    let titles: HashMap<String, String> = tickets
        .iter()
        .map(|t| (t.metadata.id.clone(), t.data.title.clone()))
        .collect();
    let graph = DependencyGraph::build(&tickets);

    let tree = render_dep_tree(&graph, &resolved, &statuses, &titles, full);
    print!("{}", tree);
    Ok(())
}

fn cmd_dep_cycle() -> Result<()> {
    let storage = make_storage(false)?;
    let tickets = storage.all()?;

    // Filter to open tickets only for cycle detection
    let open_tickets: Vec<Ticket> = tickets
        .into_iter()
        .filter(|t| t.metadata.open)
        .collect();

    let graph = DependencyGraph::build(&open_tickets);
    let cycles = graph.find_cycles();

    if cycles.is_empty() {
        println!("No dependency cycles found");
        return Ok(());
    }

    for (i, cycle) in cycles.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("Cycle {}: {}", i + 1, cycle.display);

        // Get titles for cycle members
        let statuses: HashMap<String, String> = open_tickets
            .iter()
            .map(|t| (t.metadata.id.clone(), t.metadata.status.clone()))
            .collect();
        let titles: HashMap<String, String> = open_tickets
            .iter()
            .map(|t| (t.metadata.id.clone(), t.data.title.clone()))
            .collect();

        for member in &cycle.ids {
            let s = statuses.get(member).map(|s| s.as_str()).unwrap_or("unknown");
            let t = titles.get(member).map(|t| t.as_str()).unwrap_or("(unknown)");
            println!("  {:<8} [{}] {}", member, s, t);
        }
    }
    Ok(())
}

fn format_yaml_array(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", items.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_jq_filter() {
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
---

# Test
",
            "test-0001",
        )
        .unwrap();

        assert!(matches_jq_filter(&ticket, r#".status == "open""#));
        assert!(!matches_jq_filter(&ticket, r#".status == "closed""#));
        assert!(matches_jq_filter(&ticket, r#".type == "bug""#));
        assert!(!matches_jq_filter(&ticket, r#".type == "task""#));
        assert!(matches_jq_filter(&ticket, r#".priority == 1"#));
        assert!(!matches_jq_filter(&ticket, r#".priority == 2"#));
        assert!(matches_jq_filter(&ticket, r#".assignee == "Alice""#));
        assert!(!matches_jq_filter(&ticket, r#".assignee == "Bob""#));

        // select() wrapper
        assert!(matches_jq_filter(&ticket, r#"select(.status == "open")"#));
    }
}