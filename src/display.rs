//! Human-readable output formatting for tickets.
//!
//! Provides display helpers for:
//! - Single ticket display (show)
//! - Ticket listing (ls, ready, blocked, closed)
//! - Dependency tree
//! - JSON output for query command

use crate::graph::DependencyGraph;
use crate::storage::{outline, split_file, Section, Ticket};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// What `tk show` needs from the other tickets to compute the derived sections.
pub struct ShowContext<'a> {
    pub open: &'a HashMap<String, bool>,
    pub statuses: &'a HashMap<String, String>,
    pub titles: &'a HashMap<String, String>,
    pub graph: &'a DependencyGraph,
    pub all_tickets: &'a [Ticket],
}

/// Sections `tk show` computes from other tickets (not stored in the file), in
/// print order.
pub const DERIVED_SECTIONS: [&str; 4] = ["Blockers", "Blocking", "Children", "Linked"];

/// Section names `tk show <id> <section>` accepts besides the file's own
/// headings. A file heading with the same name wins.
pub const RESERVED_SECTIONS: [&str; 7] = [
    "frontmatter",
    "title",
    "description",
    "blockers",
    "blocking",
    "children",
    "linked",
];

/// `- <id> [<status>] <title>` lines for one derived section; empty if none.
fn derived_lines(name: &str, ticket: &Ticket, ctx: &ShowContext) -> Vec<String> {
    let line = |id: &str| {
        let s = ctx.statuses.get(id).map(|s| s.as_str()).unwrap_or_default();
        let t = ctx.titles.get(id).map(|t| t.as_str()).unwrap_or("(not found)");
        format!("- {} [{}] {}", id, s, t)
    };
    let id = &ticket.metadata.id;
    match name {
        "Blockers" => ctx.graph.blockers(id, ctx.open).iter().map(|b| line(b)).collect(),
        "Blocking" => ctx
            .graph
            .blocking(id, ctx.all_tickets)
            .into_iter()
            .filter(|t| t.metadata.open)
            .map(|t| line(&t.metadata.id))
            .collect(),
        "Children" => ctx
            .graph
            .children(id, ctx.all_tickets)
            .into_iter()
            .map(|t| line(&t.metadata.id))
            .collect(),
        "Linked" => ticket.metadata.links.iter().map(|l| line(l)).collect(),
        _ => Vec::new(),
    }
}

/// The non-empty derived sections, each as `## Name` plus its lines.
fn derived_sections(ticket: &Ticket, ctx: &ShowContext) -> String {
    let mut out = String::new();
    for name in DERIVED_SECTIONS {
        let lines = derived_lines(name, ticket, ctx);
        if !lines.is_empty() {
            out.push_str(&format!("\n## {}\n{}\n", name, lines.join("\n")));
        }
    }
    out
}

fn with_newline(s: &str) -> String {
    let mut s = s.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// `tk show <id>`: the file exactly as stored, then the derived sections.
pub fn format_show_full(raw: &str, ticket: &Ticket, ctx: &ShowContext) -> String {
    with_newline(raw) + &derived_sections(ticket, ctx)
}

/// `tk show --short <id>`: frontmatter as stored, the title, then the derived
/// sections. No body.
pub fn format_show_short(raw: &str, ticket: &Ticket, ctx: &ShowContext) -> Result<String> {
    let frontmatter = split_file(raw)?.map(|p| p.frontmatter).unwrap_or_default();
    Ok(format!(
        "{}# {}\n{}",
        with_newline(frontmatter),
        ticket.data.title,
        derived_sections(ticket, ctx)
    ))
}

/// `tk show <id> <section>...`: each requested section in argument order. A
/// name matches any `##`-`######` heading in the body (case-insensitive,
/// subsections included, every duplicate printed), else a reserved name.
pub fn format_show_sections(
    raw: &str,
    ticket: &Ticket,
    ctx: &ShowContext,
    names: &[String],
) -> Result<String> {
    let parts = split_file(raw)?;
    let (frontmatter, body) = parts.map_or(("", raw), |p| (p.frontmatter, p.body));
    let lines: Vec<&str> = body.lines().collect();
    let headings: Vec<Section> = outline(body).into_iter().filter(|s| s.level >= 2).collect();

    let mut pieces: Vec<String> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for name in names {
        let want = name.trim().to_lowercase();
        let found: Vec<&Section> = headings
            .iter()
            .filter(|s| s.heading.to_lowercase() == want)
            .collect();
        if !found.is_empty() {
            for s in found {
                pieces.push(lines[s.start..s.end].join("\n").trim_end().to_string());
            }
            continue;
        }
        let derived = DERIVED_SECTIONS.iter().find(|d| d.to_lowercase() == want);
        match (want.as_str(), derived) {
            (_, Some(d)) => {
                let l = derived_lines(d, ticket, ctx);
                let body = if l.is_empty() { "(none)".to_string() } else { l.join("\n") };
                pieces.push(format!("## {}\n{}", d, body));
            }
            ("frontmatter", _) => pieces.push(frontmatter.trim_end().to_string()),
            ("title", _) => pieces.push(format!("# {}", ticket.data.title)),
            ("description", _) => {
                pieces.push(ticket.data.description.clone().unwrap_or_default())
            }
            _ => missing.push(name),
        }
    }

    if !missing.is_empty() {
        let mut available: Vec<String> = Vec::new();
        for s in &headings {
            if !available.contains(&s.heading) {
                available.push(s.heading.clone());
            }
        }
        bail!(
            "no section {} in {}\n  headings: {}\n  reserved: {}",
            missing.iter().map(|m| format!("{:?}", m)).collect::<Vec<_>>().join(", "),
            ticket.metadata.id,
            if available.is_empty() { "(none)".to_string() } else { available.join(", ") },
            RESERVED_SECTIONS.join(", ")
        );
    }
    Ok(with_newline(&pieces.join("\n\n")))
}

/// Format a ticket list entry (for ls, ready, blocked, closed).
pub fn format_ticket_list(ticket: &Ticket, show_deps: bool) -> String {
    let status_str = ticket.metadata.status.as_str();
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
    statuses: &HashMap<String, String>,
) -> String {
    let status_str = ticket.metadata.status.as_str();
    let priority_str = format_priority(ticket.metadata.priority.to_u8());

    let blocker_str: String = blockers
        .iter()
        .map(|b| {
            let s = statuses.get(b).map(|s| s.as_str()).unwrap_or_default();
            format!("{}[{}]", b, s)
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{:<8} [{}][{}] - {} <- [{}]",
        ticket.metadata.id, priority_str, status_str, ticket.data.title, blocker_str
    )
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
        r#""id":"{}","status":"{}","open":{}"#,
        json_escape(&ticket.metadata.id),
        json_escape(&ticket.metadata.status),
        ticket.metadata.open
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
        json_escape(&ticket.metadata.metadata_type)
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
    // The markdown after the frontmatter exactly as stored: every section,
    // including ones the fields above do not cover.
    parts.push(format!(r#""body":"{}""#, json_escape(&ticket.body)));

    format!("{{{}}}", parts.join(","))
}

fn json_escape(s: &str) -> String {
    // serde_json escapes every control character, not just \n \r \t.
    let quoted = serde_json::to_string(s).expect("a str always serializes");
    quoted[1..quoted.len() - 1].to_string()
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

    fn make_ticket(id: &str, title: &str, status: &str, open: bool) -> Ticket {
        Ticket::parse(
            &format!(
                "\
---
id: {}
status: {}
open: {}
deps: []
links: []
created: 2024-01-15T10:00:00Z
type: task
priority: 2
---

# {}
",
                id,
                status,
                open,
                title
            ),
            id,
        )
        .unwrap()
    }

    #[test]
    fn test_format_ticket_list() {
        let ticket = make_ticket("test-0001", "Test ticket", "open", true);
        let output = format_ticket_list(&ticket, false);
        assert!(output.contains("test-0001"));
        assert!(output.contains("[open]"));
        assert!(output.contains("Test ticket"));
        assert!(output.contains("[P2]"));
    }

    #[test]
    fn test_format_tickets_jsonl() {
        let ticket = make_ticket("test-0001", "Test ticket", "open", true);
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
        assert_eq!(json_escape("form\x0cfeed\x00"), "form\\ffeed\\u0000");
    }

    #[test]
    fn test_json_array() {
        assert_eq!(json_array(&[]), "[]");
        assert_eq!(json_array(&["a".into()]), "[\"a\"]");
        assert_eq!(json_array(&["a".into(), "b".into()]), "[\"a\",\"b\"]");
    }
}
