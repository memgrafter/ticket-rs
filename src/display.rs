//! Human-readable output formatting for tickets.
//!
//! Provides display helpers for:
//! - Single ticket display (show)
//! - Ticket listing (ls, ready, blocked, closed)
//! - Dependency tree
//! - JSON output for query command

use crate::graph::DependencyGraph;
use crate::storage::Ticket;
use crate::types::Status;
use std::collections::HashMap;

/// Format a ticket for display (like `tk show`).
pub fn format_ticket_show(
    ticket: &Ticket,
    statuses: &HashMap<String, Status>,
    titles: &HashMap<String, String>,
    graph: &DependencyGraph,
    all_tickets: &[Ticket],
) -> String {
    let mut out = String::new();

    // Frontmatter with enhanced fields
    out.push_str(&format!("id: {}\n", ticket.metadata.id));
    out.push_str(&format!(
        "status: {}\n",
        format_status(&ticket.metadata.status)
    ));
    out.push_str(&format!(
        "deps: {}\n",
        format_id_list(&ticket.metadata.deps, titles)
    ));
    out.push_str(&format!(
        "links: {}\n",
        format_id_list(&ticket.metadata.links, titles)
    ));
    out.push_str(&format!("created: {}\n", ticket.metadata.created));
    out.push_str(&format!(
        "type: {}\n",
        format_ticket_type(&ticket.metadata.metadata_type)
    ));
    out.push_str(&format!(
        "priority: {}\n",
        ticket.metadata.priority.to_u8()
    ));

    if let Some(ref a) = ticket.metadata.assignee {
        out.push_str(&format!("assignee: {}\n", a));
    }
    if let Some(ref e) = ticket.metadata.external_ref {
        out.push_str(&format!("external-ref: {}\n", e));
    }
    if let Some(ref p) = ticket.metadata.parent {
        let parent_title = titles.get(p).map(|t| t.as_str()).unwrap_or("(not found)");
        out.push_str(&format!("parent: {}  # {}\n", p, parent_title));
    }
    if let Some(ref tags) = ticket.metadata.tags {
        if !tags.is_empty() {
            out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
        }
    }

    out.push('\n');
    out.push_str(&format!("# {}\n", ticket.data.title));

    if let Some(ref desc) = ticket.data.description {
        out.push('\n');
        out.push_str(desc);
        out.push('\n');
    }

    if let Some(ref design) = ticket.data.design {
        out.push_str("\n## Design\n\n");
        out.push_str(design);
        out.push('\n');
    }

    if let Some(ref acceptance) = ticket.data.acceptance {
        out.push_str("\n## Acceptance Criteria\n\n");
        out.push_str(acceptance);
        out.push('\n');
    }

    // Blockers section
    let blockers = graph.blockers(&ticket.metadata.id, statuses);
    if !blockers.is_empty() {
        out.push_str("\n## Blockers\n");
        for b in &blockers {
            let s = statuses.get(b).map(|s| format_status(s)).unwrap_or_default();
            let t = titles.get(b).map(|t| t.as_str()).unwrap_or("(not found)");
            out.push_str(&format!("- {} [{}] {}\n", b, s, t));
        }
    }

    // Blocking section
    let blocking = graph.blocking(&ticket.metadata.id, all_tickets);
    let open_blocking: Vec<&Ticket> = blocking
        .into_iter()
        .filter(|t| t.metadata.status != Status::Closed)
        .collect();
    if !open_blocking.is_empty() {
        out.push_str("\n## Blocking\n");
        for t in &open_blocking {
            out.push_str(&format!(
                "- {} [{}] {}\n",
                t.metadata.id,
                format_status(&t.metadata.status),
                t.data.title
            ));
        }
    }

    // Children section
    let children = graph.children(&ticket.metadata.id, all_tickets);
    if !children.is_empty() {
        out.push_str("\n## Children\n");
        for t in &children {
            out.push_str(&format!(
                "- {} [{}] {}\n",
                t.metadata.id,
                format_status(&t.metadata.status),
                t.data.title
            ));
        }
    }

    // Linked section
    let linked: Vec<&Ticket> = all_tickets
        .iter()
        .filter(|t| ticket.metadata.links.contains(&t.metadata.id))
        .collect();
    if !linked.is_empty() {
        out.push_str("\n## Linked\n");
        for t in &linked {
            out.push_str(&format!(
                "- {} [{}] {}\n",
                t.metadata.id,
                format_status(&t.metadata.status),
                t.data.title
            ));
        }
    }

    // Notes section
    if let Some(ref notes) = ticket.data.notes {
        if !notes.is_empty() {
            out.push_str("\n## Notes\n");
            for note in notes {
                out.push_str(&format!("\n**{}**\n\n{}\n", note.timestamp, note.body));
            }
        }
    }

    out
}

/// Format a ticket list entry (for ls, ready, blocked, closed).
pub fn format_ticket_list(ticket: &Ticket, show_deps: bool) -> String {
    let status_str = format_status(&ticket.metadata.status);
    let priority_str = format_priority(ticket.metadata.priority.to_u8());

    let deps_str = if show_deps && !ticket.metadata.deps.is_empty() {
        format!(" <- [{}]", ticket.metadata.deps.join(", "))
    } else {
        String::new()
    };

    format!(
        "{:<8} [{}][{}] - {}{}",
        ticket.metadata.id, priority_str, status_str, ticket.data.title, deps_str
    )
}

/// Format a blocked ticket entry, showing blockers.
pub fn format_ticket_blocked(
    ticket: &Ticket,
    blockers: &[String],
    statuses: &HashMap<String, Status>,
) -> String {
    let status_str = format_status(&ticket.metadata.status);
    let priority_str = format_priority(ticket.metadata.priority.to_u8());

    let blocker_str: String = blockers
        .iter()
        .map(|b| {
            let s = statuses.get(b).map(|s| format_status(s)).unwrap_or_default();
            format!("{}[{}]", b, s)
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{:<8} [{}][{}] - {} <- [{}]",
        ticket.metadata.id, priority_str, status_str, ticket.data.title, blocker_str
    )
}

/// Format a list of IDs with their titles in parentheses.
fn format_id_list(ids: &[String], titles: &HashMap<String, String>) -> String {
    if ids.is_empty() {
        return "[]".to_string();
    }

    let items: Vec<String> = ids
        .iter()
        .map(|id| {
            let title = titles.get(id).map(|t| t.as_str()).unwrap_or("(unknown)");
            format!("{} ({})", id, title)
        })
        .collect();

    format!("[{}]", items.join(", "))
}

/// Format a status for display.
pub fn format_status(status: &Status) -> &'static str {
    match status {
        Status::Open => "open",
        Status::InProgress => "in_progress",
        Status::Closed => "closed",
    }
}

/// Format a ticket type for display.
pub fn format_ticket_type(t: &crate::types::TicketType) -> &'static str {
    match t {
        crate::types::TicketType::Bug => "bug",
        crate::types::TicketType::Feature => "feature",
        crate::types::TicketType::Task => "task",
        crate::types::TicketType::Epic => "epic",
        crate::types::TicketType::Chore => "chore",
    }
}

/// Format a priority number for display.
pub fn format_priority(p: u8) -> String {
    format!("P{}", p)
}

/// Format tickets as JSONL (one JSON object per line).
pub fn format_tickets_jsonl(tickets: &[Ticket]) -> String {
    tickets
        .iter()
        .map(ticket_to_json)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Serialize a single ticket to JSON string.
fn ticket_to_json(ticket: &Ticket) -> String {
    // Manual JSON construction to match the format from the bash script
    let mut parts = Vec::new();

    parts.push(format!(
        r#""id":"{}","status":"{}""#,
        json_escape(&ticket.metadata.id),
        format_status(&ticket.metadata.status)
    ));
    parts.push(format!(
        r#""deps":{}"#,
        json_array(&ticket.metadata.deps)
    ));
    parts.push(format!(
        r#""links":{}"#,
        json_array(&ticket.metadata.links)
    ));
    parts.push(format!(
        r#""created":"{}""#,
        json_escape(&ticket.metadata.created)
    ));
    parts.push(format!(
        r#""type":"{}""#,
        format_ticket_type(&ticket.metadata.metadata_type)
    ));
    parts.push(format!(
        r#""priority":{}"#,
        ticket.metadata.priority.to_u8()
    ));

    if let Some(ref a) = ticket.metadata.assignee {
        parts.push(format!(r#""assignee":"{}""#, json_escape(a)));
    }
    if let Some(ref e) = ticket.metadata.external_ref {
        parts.push(format!(r#""externalRef":"{}""#, json_escape(e)));
    }
    if let Some(ref p) = ticket.metadata.parent {
        parts.push(format!(r#""parent":"{}""#, json_escape(p)));
    }
    if let Some(ref tags) = ticket.metadata.tags {
        if !tags.is_empty() {
            parts.push(format!(r#""tags":{}"#, json_array(tags)));
        }
    }

    parts.push(format!(r#""title":"{}""#, json_escape(&ticket.data.title)));

    if let Some(ref desc) = ticket.data.description {
        parts.push(format!(r#""description":"{}""#, json_escape(desc)));
    }

    format!("{{{}}}", parts.join(","))
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn json_array(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        let inner: Vec<String> = items.iter().map(|s| format!("\"{}\"", json_escape(s))).collect();
        format!("[{}]", inner.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Ticket;

    fn make_ticket(id: &str, title: &str, status: Status) -> Ticket {
        Ticket::parse(
            &format!(
                "\
---
id: {}
status: {}
deps: []
links: []
created: 2024-01-15T10:00:00Z
type: task
priority: 2
---

# {}
",
                id,
                match status {
                    Status::Open => "open",
                    Status::InProgress => "in_progress",
                    Status::Closed => "closed",
                },
                title
            ),
            id,
        )
        .unwrap()
    }

    #[test]
    fn test_format_ticket_list() {
        let ticket = make_ticket("test-0001", "Test ticket", Status::Open);
        let output = format_ticket_list(&ticket, false);
        assert!(output.contains("test-0001"));
        assert!(output.contains("[open]"));
        assert!(output.contains("Test ticket"));
        assert!(output.contains("[P2]"));
    }

    #[test]
    fn test_format_status() {
        assert_eq!(format_status(&Status::Open), "open");
        assert_eq!(format_status(&Status::InProgress), "in_progress");
        assert_eq!(format_status(&Status::Closed), "closed");
    }

    #[test]
    fn test_format_tickets_jsonl() {
        let ticket = make_ticket("test-0001", "Test ticket", Status::Open);
        let jsonl = format_tickets_jsonl(&[ticket]);
        assert!(jsonl.contains("test-0001"));
        assert!(jsonl.contains("open"));
        assert!(jsonl.contains("Test ticket"));
        assert!(jsonl.contains("priority"));

        // Should be valid JSON on each line
        for line in jsonl.lines() {
            assert!(line.starts_with('{'));
            assert!(line.ends_with('}'));
        }
    }

    #[test]
    fn test_json_escape() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("hello\"world"), "hello\\\"world");
        assert_eq!(json_escape("hello\nworld"), "hello\\nworld");
    }

    #[test]
    fn test_json_array() {
        assert_eq!(json_array(&[]), "[]");
        assert_eq!(json_array(&["a".into()]), "[\"a\"]");
        assert_eq!(json_array(&["a".into(), "b".into()]), "[\"a\",\"b\"]");
    }
}