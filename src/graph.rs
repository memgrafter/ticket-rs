//! Dependency graph operations for tickets.
//!
//! Provides:
//! - Building a dependency graph from a set of tickets
//! - Topological sorting and depth computation
//! - Cycle detection
//! - Dependency tree rendering (with box-drawing characters)
//! - Computing inverse relationships (blocked_by, blocking, children)

use crate::storage::Ticket;
use crate::types::Status;
use std::collections::{HashMap, HashSet, VecDeque};

/// Directed graph of ticket dependencies.
///
/// `A -> B` means A depends on B (B is a dependency of A).
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    /// Adjacency list: node -> list of dependencies (outgoing edges).
    pub deps: HashMap<String, Vec<String>>,
    /// Adjacency list: node -> list of dependents (incoming edges).
    pub dependents: HashMap<String, Vec<String>>,
}

/// Result of a cycle detection.
#[derive(Debug, Clone)]
pub struct Cycle {
    /// The cycle as a list of ticket IDs.
    pub ids: Vec<String>,
    /// Formatted cycle string like "A -> B -> C -> A".
    pub display: String,
}

#[allow(dead_code)]
impl DependencyGraph {
    /// Build a dependency graph from a list of tickets.
    pub fn build(tickets: &[Ticket]) -> Self {
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

        for ticket in tickets {
            let id = &ticket.metadata.id;
            deps.entry(id.clone()).or_default();
            dependents.entry(id.clone()).or_default();

            for dep_id in &ticket.metadata.deps {
                deps.get_mut(id).unwrap().push(dep_id.clone());
                dependents.entry(dep_id.clone()).or_default().push(id.clone());
            }
        }

        DependencyGraph { deps, dependents }
    }

    /// Get the dependencies of a node.
    pub fn get_deps(&self, id: &str) -> &[String] {
        self.deps.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get the dependents of a node.
    pub fn get_dependents(&self, id: &str) -> &[String] {
        self.dependents.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Check if a ticket has all dependencies closed.
    pub fn is_ready(&self, id: &str, statuses: &HashMap<String, Status>) -> bool {
        if let Some(deps) = self.deps.get(id) {
            for dep_id in deps {
                if statuses.get(dep_id).map(|s| *s != Status::Closed).unwrap_or(true) {
                    return false;
                }
            }
        }
        true
    }

    /// Check if a ticket has at least one non-closed dependency.
    pub fn is_blocked(&self, id: &str, statuses: &HashMap<String, Status>) -> bool {
        !self.is_ready(id, statuses) && self.deps.get(id).map(|d| !d.is_empty()).unwrap_or(false)
    }

    /// Get the list of blocker tickets (non-closed dependencies).
    pub fn blockers(&self, id: &str, statuses: &HashMap<String, Status>) -> Vec<String> {
        self.deps
            .get(id)
            .map(|deps| {
                deps.iter()
                    .filter(|d| statuses.get(d.as_str()).map(|s| *s != Status::Closed).unwrap_or(true))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Detect cycles in the dependency graph.
    ///
    /// Uses DFS with white/gray/black coloring.
    pub fn find_cycles(&self) -> Vec<Cycle> {
        // Only consider tickets present in the graph
        let nodes: HashSet<&String> = self.deps.keys().chain(self.dependents.keys()).collect();
        let mut state: HashMap<&String, u8> = HashMap::new(); // 0=white, 1=gray, 2=black
        let mut cycles = Vec::new();
        let mut seen_normalized = HashSet::new();

        for node in &nodes {
            if *state.get(node).unwrap_or(&0) != 0 {
                continue;
            }

            // Iterative DFS with path tracking
            let mut stack: Vec<(&String, Vec<&String>)> = vec![(node, vec![])];
            let _path: Vec<&String> = Vec::new();

            while let Some((current, current_path)) = stack.pop() {
                // Check if we're backtracking to a node we've fully processed
                let st = state.entry(current).or_insert(0);
                if *st == 2 {
                    continue;
                }

                if *st == 1 {
                    // Found cycle
                    let cycle_start_idx = current_path.iter().position(|n| *n == current);
                    if let Some(start) = cycle_start_idx {
                        let cycle_ids: Vec<String> = current_path[start..]
                            .iter()
                            .map(|s| (*s).clone())
                            .collect();

                        // Build normalized cycle string (starting from smallest ID)
                        let display = format!("{} -> {}", cycle_ids.join(" -> "), cycle_ids[0]);
                        let normalized = normalize_cycle(&cycle_ids);

                        if seen_normalized.insert(normalized) {
                            cycles.push(Cycle {
                                ids: cycle_ids,
                                display,
                            });
                        }
                    }
                    continue;
                }

                // Mark gray
                *st = 1;

                // Push backtrack marker
                stack.push((current, current_path.clone()));

                // Push children
                if let Some(deps) = self.deps.get(current) {
                    for dep in deps.iter().rev() {
                        let dep_state = state.get(dep).copied().unwrap_or(0);
                        if dep_state == 2 {
                            continue;
                        }
                        let mut new_path = current_path.clone();
                        new_path.push(current);
                        stack.push((dep, new_path));
                    }
                }
            }

            // Mark all reachable as black
            for n in &nodes {
                if *state.get(n).unwrap_or(&0) == 1 {
                    state.insert(n, 2);
                }
            }
        }

        cycles
    }

    /// Check if the graph has any cycles.
    pub fn has_cycles(&self) -> bool {
        !self.find_cycles().is_empty()
    }

    /// Compute subtree depths (max depth from this node to any leaf).
    pub fn subtree_depths(&self, root: &str) -> HashMap<String, u32> {
        let mut depths: HashMap<String, u32> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();

        // Post-order traversal using stack
        let mut stack: Vec<(&str, bool)> = vec![(root, false)];

        while let Some((node, processed)) = stack.pop() {
            if processed {
                // Compute depth: max child depth + 1
                let max_child = self
                    .deps
                    .get(node)
                    .map(|deps| {
                        deps.iter()
                            .filter(|c| depths.contains_key(c.as_str()))
                            .map(|c| depths[c])
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                depths.insert(node.to_string(), max_child + 1);
                continue;
            }

            if visited.contains(node) {
                continue;
            }
            visited.insert(node.to_string());

            stack.push((node, true));
            if let Some(deps) = self.deps.get(node) {
                for dep in deps.iter().rev() {
                    if !visited.contains(dep.as_str()) {
                        stack.push((dep.as_str(), false));
                    }
                }
            }
        }

        depths
    }

    /// Get children of a node (where parent field points to the node).
    pub fn children<'a>(&self, id: &str, tickets: &'a [Ticket]) -> Vec<&'a Ticket> {
        tickets
            .iter()
            .filter(|t| t.metadata.parent.as_deref() == Some(id))
            .collect()
    }

    /// Get tickets that this ticket blocks (tickets that depend on this one).
    pub fn blocking<'a>(&self, id: &str, tickets: &'a [Ticket]) -> Vec<&'a Ticket> {
        tickets
            .iter()
            .filter(|t| t.metadata.deps.contains(&id.to_string()))
            .collect()
    }
}

/// Normalize a cycle by starting from the smallest ID (lexicographically).
fn normalize_cycle(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }

    let min_idx = ids
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.cmp(b))
        .map(|(i, _)| i)
        .unwrap_or(0);

    let normalized: Vec<&str> = (0..ids.len())
        .map(|i| ids[(min_idx + i) % ids.len()].as_str())
        .collect();

    normalized.join(",")
}

/// Render a dependency tree as a string with box-drawing characters.
pub fn render_dep_tree(
    graph: &DependencyGraph,
    root: &str,
    statuses: &HashMap<String, Status>,
    titles: &HashMap<String, String>,
    full: bool,
) -> String {
    let _depths = graph.subtree_depths(root);
    let mut output = String::new();
    let mut visited: HashSet<String> = HashSet::new();

    // Root line
    let root_status = statuses.get(root).map(|s| format!("{:?}", s)).unwrap_or_default();
    let root_title = titles.get(root).map(|s| s.as_str()).unwrap_or("");
    output.push_str(&format!("{} [{}] {}\n", root, root_status, root_title));

    visited.insert(root.to_string());

    // Stack entries: (node_id, depth, prefix_string, connector)
    #[derive(Clone)]
    struct StackEntry {
        id: String,
        prefix: String,
        last: bool,
    }

    let mut stack: VecDeque<StackEntry> = VecDeque::new();

    // Start with root's children
    if let Some(children) = graph.deps.get(root) {
        let printable: Vec<&String> = children
            .iter()
            .filter(|c| full || !visited.contains(c.as_str()))
            .collect();

        for (i, child) in printable.iter().enumerate() {
            let last = i == printable.len() - 1;
            stack.push_back(StackEntry {
                id: (*child).clone(),
                prefix: if last { "    ".into() } else { "│   ".into() },
                last,
            });
        }
    }

    while let Some(entry) = stack.pop_front() {
        if !full && visited.contains(&entry.id) {
            continue;
        }
        visited.insert(entry.id.clone());

        let connector = if entry.last { "└── " } else { "├── " };
        let status = statuses
            .get(&entry.id)
            .map(|s| format!("{:?}", s))
            .unwrap_or_default();
        let title = titles.get(&entry.id).map(|s| s.as_str()).unwrap_or("");

        output.push_str(&format!(
            "{}{} [{}] {}\n",
            entry.prefix, connector, status, title
        ));

        // Push children
        if let Some(children) = graph.deps.get(&entry.id) {
            let printable: Vec<&String> = children
                .iter()
                .filter(|c| full || !visited.contains(c.as_str()))
                .collect();

            let child_count = printable.len();
            for (i, child) in printable.iter().enumerate() {
                let is_last = i == child_count - 1;
                let child_prefix = if entry.last {
                    format!("{}    ", entry.prefix)
                } else {
                    format!("{}│   ", entry.prefix)
                };

                // Insert after current position
                let se = StackEntry {
                    id: (*child).clone(),
                    prefix: child_prefix,
                    last: is_last,
                };
                stack.push_front(se);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Ticket;
    use crate::types::{Metadata, Status, TicketType, Priority, Data};

    fn make_ticket(id: &str, deps: Vec<String>, status: Status, parent: Option<String>) -> Ticket {
        Ticket {
            metadata: Metadata {
                id: id.to_string(),
                status,
                deps,
                links: vec![],
                created: "2024-01-15T10:00:00Z".to_string(),
                metadata_type: TicketType::Task,
                priority: Priority::P2,
                assignee: None,
                external_ref: None,
                parent,
                tags: None,
            },
            data: Data {
                title: format!("Ticket {}", id),
                description: None,
                design: None,
                acceptance: None,
                notes: None,
            },
        }
    }

    fn build_statuses(tickets: &[Ticket]) -> HashMap<String, Status> {
        tickets.iter().map(|t| (t.metadata.id.clone(), t.metadata.status)).collect()
    }

    fn build_titles(tickets: &[Ticket]) -> HashMap<String, String> {
        tickets.iter().map(|t| (t.metadata.id.clone(), t.data.title.clone())).collect()
    }

    #[test]
    fn test_graph_build() {
        let tickets = vec![
            make_ticket("A", vec!["B".into()], Status::Open, None),
            make_ticket("B", vec![], Status::Open, None),
            make_ticket("C", vec!["A".into()], Status::Closed, None),
        ];

        let graph = DependencyGraph::build(&tickets);

        assert_eq!(graph.get_deps("A"), &["B"]);
        assert_eq!(graph.get_deps("B"), &[] as &[String]);
        assert_eq!(graph.get_deps("C"), &["A"]);

        // Dependents
        assert_eq!(graph.get_dependents("A"), &["C"]);
        assert_eq!(graph.get_dependents("B"), &["A"]);
    }

    #[test]
    fn test_is_ready() {
        let tickets = vec![
            make_ticket("A", vec!["B".into()], Status::Open, None),
            make_ticket("B", vec![], Status::Closed, None),
            make_ticket("C", vec!["D".into()], Status::Open, None),
            make_ticket("D", vec![], Status::Open, None),
        ];

        let statuses = build_statuses(&tickets);
        let graph = DependencyGraph::build(&tickets);

        assert!(graph.is_ready("A", &statuses)); // dep B is closed
        assert!(!graph.is_ready("C", &statuses)); // dep D is open
        assert!(graph.is_ready("B", &statuses)); // no deps
    }

    #[test]
    fn test_is_blocked() {
        let tickets = vec![
            make_ticket("A", vec!["B".into()], Status::Open, None),
            make_ticket("B", vec![], Status::Closed, None),
            make_ticket("C", vec!["D".into()], Status::Open, None),
            make_ticket("D", vec![], Status::Open, None),
            make_ticket("E", vec![], Status::Open, None),
        ];

        let statuses = build_statuses(&tickets);
        let graph = DependencyGraph::build(&tickets);

        assert!(!graph.is_blocked("A", &statuses)); // dep B is closed
        assert!(graph.is_blocked("C", &statuses));  // dep D is open
        assert!(!graph.is_blocked("E", &statuses)); // no deps
    }

    #[test]
    fn test_blockers() {
        let tickets = vec![
            make_ticket("A", vec!["B".into(), "C".into()], Status::Open, None),
            make_ticket("B", vec![], Status::Closed, None),
            make_ticket("C", vec![], Status::Open, None),
        ];

        let statuses = build_statuses(&tickets);
        let graph = DependencyGraph::build(&tickets);

        let blockers = graph.blockers("A", &statuses);
        assert_eq!(blockers, vec!["C"]);
    }

    #[test]
    fn test_cycle_detection_simple() {
        let tickets = vec![
            make_ticket("A", vec!["B".into()], Status::Open, None),
            make_ticket("B", vec!["A".into()], Status::Open, None),
        ];

        let graph = DependencyGraph::build(&tickets);
        let cycles = graph.find_cycles();
        assert!(!cycles.is_empty());
        assert!(cycles[0].display.contains("A"));
        assert!(cycles[0].display.contains("B"));
    }

    #[test]
    fn test_no_cycles() {
        let tickets = vec![
            make_ticket("A", vec!["B".into()], Status::Open, None),
            make_ticket("B", vec!["C".into()], Status::Open, None),
            make_ticket("C", vec![], Status::Open, None),
        ];

        let graph = DependencyGraph::build(&tickets);
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn test_cycle_detection_longer() {
        let tickets = vec![
            make_ticket("A", vec!["B".into()], Status::Open, None),
            make_ticket("B", vec!["C".into()], Status::Open, None),
            make_ticket("C", vec!["A".into()], Status::Open, None),
        ];

        let graph = DependencyGraph::build(&tickets);
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].ids.len(), 3);
    }

    #[test]
    fn test_subtree_depths() {
        let tickets = vec![
            make_ticket("A", vec!["B".into(), "C".into()], Status::Open, None),
            make_ticket("B", vec!["D".into()], Status::Open, None),
            make_ticket("C", vec![], Status::Open, None),
            make_ticket("D", vec![], Status::Open, None),
        ];

        let graph = DependencyGraph::build(&tickets);
        let depths = graph.subtree_depths("A");

        // A: max child depth + 1 = 3
        // B: depth 2 (D is 1)
        // C: depth 1
        // D: depth 1
        assert_eq!(depths.get("A"), Some(&3));
        assert_eq!(depths.get("B"), Some(&2));
        assert_eq!(depths.get("C"), Some(&1));
        assert_eq!(depths.get("D"), Some(&1));
    }

    #[test]
    fn test_render_dep_tree() {
        let tickets = vec![
            make_ticket("A", vec!["B".into(), "C".into()], Status::Open, None),
            make_ticket("B", vec!["D".into()], Status::Open, None),
            make_ticket("C", vec![], Status::Closed, None),
            make_ticket("D", vec![], Status::Open, None),
        ];

        let graph = DependencyGraph::build(&tickets);
        let statuses = build_statuses(&tickets);
        let titles = build_titles(&tickets);

        let tree = render_dep_tree(&graph, "A", &statuses, &titles, false);
        assert!(tree.contains("A"));
        assert!(tree.contains("B"));
        assert!(tree.contains("C"));
        assert!(tree.contains("D"));

        // Check box-drawing characters
        assert!(tree.contains("├──") || tree.contains("└──"));
    }
}