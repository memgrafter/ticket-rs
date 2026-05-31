//! Integration tests for ticket-rs.
//!
//! These test the CLI binary end-to-end by running `cargo run` with various args
//! inside temporary directories.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::str;

/// Get the path to the built binary.
fn binary_path() -> PathBuf {
    // We need to find the binary. During `cargo test`, the binary is built.
    // Use CARGO_MANIFEST_DIR to find it.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.join("target").join("debug");
    target_dir.join("ticket-rs")
}

/// Helper to run the ticket binary with args in a given working directory.
fn tk(dir: &Path, args: &[&str]) -> Output {
    let bin = binary_path();
    Command::new(&bin)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run tk command")
}

/// Helper to run the ticket binary with args, capturing stdout as string.
fn tk_stdout(dir: &Path, args: &[&str]) -> String {
    let out = tk(dir, args);
    assert!(out.status.success(), "tk {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Helper to run the ticket binary expecting failure, capturing stderr.
fn tk_fail(dir: &Path, args: &[&str]) -> String {
    let out = tk(dir, args);
    assert!(!out.status.success(), "tk {:?} unexpectedly succeeded", args);
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

/// Create a helper struct for integration tests.
struct TicketTest {
    dir: tempfile::TempDir,
}

impl TicketTest {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        TicketTest { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str]) -> Output {
        tk(self.path(), args)
    }

    fn run_ok(&self, args: &[&str]) -> String {
        tk_stdout(self.path(), args)
    }

    fn run_fail(&self, args: &[&str]) -> String {
        tk_fail(self.path(), args)
    }

    /// Create a ticket and return its ID.
    fn create(&self, title: &str) -> String {
        let out = self.run_ok(&["create", title]);
        // Output should be a ticket ID
        assert!(!out.is_empty(), "create returned empty ID");
        assert!(
            out.contains('-'),
            "create returned invalid ID (no hyphen): {}",
            out
        );
        out
    }

    /// Read a ticket file and return its content.
    fn read_ticket_file(&self, id: &str) -> String {
        let path = self.path().join(".tickets").join(format!("{}.md", id));
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read ticket file {:?}: {}", path, e))
    }

    /// Assert that a ticket file exists.
    fn assert_ticket_exists(&self, id: &str) {
        let path = self.path().join(".tickets").join(format!("{}.md", id));
        assert!(path.exists(), "Ticket file {:?} does not exist", path);
    }
}

// ---------------------------------------------------------------------------
// Creation tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_basic_ticket() {
    let t = TicketTest::new();
    let id = t.create("My first ticket");
    let content = t.read_ticket_file(&id);
    assert!(content.contains(&id), "Content should contain the ID");
    assert!(content.contains("# My first ticket"), "Content should contain title");
    assert!(content.contains("status: open"), "Default status should be open");
    assert!(content.contains("type: task"), "Default type should be task");
    assert!(content.contains("priority: 2"), "Default priority should be 2");
    assert!(content.contains("deps: []"), "Default deps should be empty");
    assert!(content.contains("links: []"), "Default links should be empty");
    // Should have a created timestamp
    assert!(content.contains("created: "), "Should have a created timestamp");
}

#[test]
fn test_create_with_title() {
    let t = TicketTest::new();
    let id = t.create("Test ticket");
    let content = t.read_ticket_file(&id);
    assert!(content.contains("# Test ticket"));
}

#[test]
fn test_create_default_title() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("# Untitled"));
}

#[test]
fn test_create_with_description() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "Test ticket", "-d", "This is the description"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("This is the description"));
}

#[test]
fn test_create_with_type() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "Bug ticket", "-t", "bug"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("type: bug"));
}

#[test]
fn test_create_with_priority() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "High priority", "-p", "0"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("priority: 0"));
}

#[test]
fn test_create_with_assignee() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "Assigned ticket", "-a", "John Doe"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("assignee: John Doe"));
}

#[test]
fn test_create_with_external_ref() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "External", "--external-ref", "JIRA-123"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("external-ref: JIRA-123"));
}

#[test]
fn test_create_with_design() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "Design ticket", "--design", "Use microservices"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("## Design"));
    assert!(content.contains("Use microservices"));
}

#[test]
fn test_create_with_acceptance() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "Story", "--acceptance", "Should pass all tests"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("## Acceptance Criteria"));
    assert!(content.contains("Should pass all tests"));
}

#[test]
fn test_create_with_tags() {
    let t = TicketTest::new();
    let id = t.run_ok(&["create", "Tagged", "--tags", "ui,backend,urgent"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("ui, backend, urgent") || content.contains("ui") && content.contains("urgent"));
}

// ---------------------------------------------------------------------------
// Status tests
// ---------------------------------------------------------------------------

#[test]
fn test_status_in_progress() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["status", &id, "in_progress"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("status: in_progress"));
}

#[test]
fn test_status_closed() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["status", &id, "closed"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("status: closed"));
}

#[test]
fn test_status_reopen() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["status", &id, "closed"]);
    t.run_ok(&["status", &id, "open"]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("status: open"));
}

#[test]
fn test_start_command() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["start", &id]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("status: in_progress"));
}

#[test]
fn test_close_command() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["close", &id]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("status: closed"));
}

#[test]
fn test_reopen_command() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["close", &id]);
    t.run_ok(&["reopen", &id]);
    let content = t.read_ticket_file(&id);
    assert!(content.contains("status: open"));
}

#[test]
fn test_invalid_status() {
    let t = TicketTest::new();
    let id = t.create("Test");
    let err = t.run_fail(&["status", &id, "invalid"]);
    assert!(err.contains("invalid status") || err.contains("invalid"));
}

#[test]
fn test_status_non_existent() {
    let t = TicketTest::new();
    let err = t.run_fail(&["status", "nonexistent", "open"]);
    assert!(err.contains("not found") || err.contains("Error"));
}

// ---------------------------------------------------------------------------
// Show tests
// ---------------------------------------------------------------------------

#[test]
fn test_show_ticket() {
    let t = TicketTest::new();
    let id = t.create("Test ticket");
    let out = t.run_ok(&["show", &id]);
    assert!(out.contains(&id));
    assert!(out.contains("Test ticket"));
    assert!(out.contains("status: open"));
}

#[test]
fn test_show_non_existent() {
    let t = TicketTest::new();
    let err = t.run_fail(&["show", "nonexistent"]);
    assert!(err.contains("not found") || err.contains("Error"));
}

#[test]
fn test_show_with_partial_id() {
    let t = TicketTest::new();
    let id = t.create("Test ticket");
    // Extract suffix for partial match
    let suffix = id.split('-').nth(1).unwrap();
    let out = t.run_ok(&["show", suffix]);
    assert!(out.contains(&id));
}

// ---------------------------------------------------------------------------
// List tests
// ---------------------------------------------------------------------------

#[test]
fn test_list_all() {
    let t = TicketTest::new();
    let id1 = t.create("First ticket");
    let id2 = t.create("Second ticket");
    let out = t.run_ok(&["ls"]);
    assert!(out.contains(&id1));
    assert!(out.contains(&id2));
}

#[test]
fn test_list_empty() {
    let t = TicketTest::new();
    // Create a ticket so directory is initialized, then try ls
    t.create("temp");
    let out = t.run_ok(&["ls"]);
    // After create+ls, should show the ticket we created
    assert!(!out.is_empty(), "Expected at least one ticket");
}

#[test]
fn test_list_with_status_filter() {
    let t = TicketTest::new();
    let id1 = t.create("Open ticket");
    let id2 = t.create("Closed ticket");
    t.run_ok(&["close", &id2]);

    let out = t.run_ok(&["ls", "--status", "open"]);
    assert!(out.contains(&id1), "Open ticket should appear: {}", out);
    assert!(!out.contains(&id2), "Closed ticket should not appear: {}", out);
}

// ---------------------------------------------------------------------------
// Dependency tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_dependency() {
    let t = TicketTest::new();
    let id1 = t.create("Main ticket");
    let id2 = t.create("Dependency ticket");
    let out = t.run_ok(&["dep", &id1, &id2]);
    assert!(out.contains("Added dependency") || out.contains("->"));

    let content = t.read_ticket_file(&id1);
    assert!(content.contains(&id2), "Deps should contain the dependency ID");
}

#[test]
fn test_add_dependency_idempotent() {
    let t = TicketTest::new();
    let id1 = t.create("Main ticket");
    let id2 = t.create("Dependency ticket");
    t.run_ok(&["dep", &id1, &id2]);
    let out = t.run_ok(&["dep", &id1, &id2]);
    assert!(out.contains("already exists"), "Expected 'already exists', got: {}", out);
}

#[test]
fn test_remove_dependency() {
    let t = TicketTest::new();
    let id1 = t.create("Main ticket");
    let id2 = t.create("Dependency ticket");
    t.run_ok(&["dep", &id1, &id2]);
    let out = t.run_ok(&["undep", &id1, &id2]);
    assert!(out.contains("Removed dependency") || out.contains("-/->"));

    let content = t.read_ticket_file(&id1);
    assert!(!content.contains(&format!("[{}]", id2)), "Deps should not contain the removed dependency");
}

#[test]
fn test_remove_non_existent_dependency() {
    let t = TicketTest::new();
    let id1 = t.create("Main ticket");
    let id2 = t.create("Dependency ticket");
    let err = t.run_fail(&["undep", &id1, &id2]);
    assert!(err.contains("not found"));
}

// ---------------------------------------------------------------------------
// Link tests
// ---------------------------------------------------------------------------

#[test]
fn test_link_two_tickets() {
    let t = TicketTest::new();
    let id1 = t.create("First");
    let id2 = t.create("Second");
    let out = t.run_ok(&["link", &id1, &id2]);
    assert!(out.contains("link") || out.contains("Added"));

    let content1 = t.read_ticket_file(&id1);
    let content2 = t.read_ticket_file(&id2);
    assert!(content1.contains(&id2), "First should link to second");
    assert!(content2.contains(&id1), "Second should link to first");
}

#[test]
fn test_unlink() {
    let t = TicketTest::new();
    let id1 = t.create("First");
    let id2 = t.create("Second");
    t.run_ok(&["link", &id1, &id2]);
    t.run_ok(&["unlink", &id1, &id2]);

    let content1 = t.read_ticket_file(&id1);
    let content2 = t.read_ticket_file(&id2);
    assert!(!content1.contains(&id2) || content1.contains("links: []"),
        "First should have no links after unlink: {}", content1);
}

// ---------------------------------------------------------------------------
// Ready / Blocked / Closed tests
// ---------------------------------------------------------------------------

#[test]
fn test_ready_shows_ticket_without_deps() {
    let t = TicketTest::new();
    let id = t.create("Ready ticket");
    let out = t.run_ok(&["ready"]);
    assert!(out.contains(&id));
}

#[test]
fn test_ready_excludes_blocked_ticket() {
    let t = TicketTest::new();
    let id1 = t.create("Blocked ticket");
    let id2 = t.create("Blocker");
    t.run_ok(&["dep", &id1, &id2]);
    let out = t.run_ok(&["ready"]);
    assert!(!out.contains(&id1), "Blocked ticket should not show in ready: {}", out);
    assert!(out.contains(&id2), "Blocker should show in ready: {}", out);
}

#[test]
fn test_ready_includes_ticket_with_closed_deps() {
    let t = TicketTest::new();
    let id1 = t.create("Main");
    let id2 = t.create("Done blocker");
    t.run_ok(&["dep", &id1, &id2]);
    t.run_ok(&["close", &id2]);

    let out = t.run_ok(&["ready"]);
    assert!(out.contains(&id1), "Ticket with closed deps should be ready: {}", out);
}

#[test]
fn test_ready_excludes_closed() {
    let t = TicketTest::new();
    let id = t.create("Closed ticket");
    t.run_ok(&["close", &id]);
    let out = t.run_ok(&["ready"]);
    assert!(!out.contains(&id), "Closed ticket should not be ready: {}", out);
}

#[test]
fn test_blocked_shows_ticket_with_open_deps() {
    let t = TicketTest::new();
    let id1 = t.create("Blocked ticket");
    let id2 = t.create("Blocker");
    t.run_ok(&["dep", &id1, &id2]);
    let out = t.run_ok(&["blocked"]);
    assert!(out.contains(&id1), "Blocked ticket should appear: {}", out);
    assert!(out.contains(&id2) || out.contains("<-"), "Should show blocker: {}", out);
}

#[test]
fn test_closed_shows_recently_closed() {
    let t = TicketTest::new();
    let id = t.create("Done ticket");
    t.run_ok(&["close", &id]);
    let out = t.run_ok(&["closed"]);
    assert!(out.contains(&id), "Closed ticket should appear: {}", out);
    assert!(out.contains("closed"), "Should show closed status");
}

// ---------------------------------------------------------------------------
// Note tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_note() {
    let t = TicketTest::new();
    let id = t.create("Test");
    let out = t.run_ok(&["add-note", &id, "This is my note"]);
    assert!(out.contains("Note added"));

    let content = t.read_ticket_file(&id);
    assert!(content.contains("## Notes"), "Should have Notes section");
    assert!(content.contains("This is my note"), "Should contain note text");
    assert!(content.contains("**"), "Should have timestamp markers");
}

#[test]
fn test_add_multiple_notes() {
    let t = TicketTest::new();
    let id = t.create("Test");
    t.run_ok(&["add-note", &id, "First note"]);
    t.run_ok(&["add-note", &id, "Second note"]);

    let content = t.read_ticket_file(&id);
    assert!(content.contains("First note"));
    assert!(content.contains("Second note"));
}

#[test]
fn test_add_note_non_existent() {
    let t = TicketTest::new();
    let err = t.run_fail(&["add-note", "nonexistent", "My note"]);
    assert!(err.contains("not found") || err.contains("Error"));
}

// ---------------------------------------------------------------------------
// Query tests
// ---------------------------------------------------------------------------

#[test]
fn test_query_all() {
    let t = TicketTest::new();
    let id1 = t.create("First");
    let id2 = t.create("Second");
    let out = t.run_ok(&["query"]);
    assert!(out.contains(&id1));
    assert!(out.contains(&id2));
    // Each line should be valid JSON
    for line in out.lines() {
        assert!(line.starts_with('{'), "Each line should be JSON: {}", line);
    }
}

#[test]
fn test_query_with_filter() {
    let t = TicketTest::new();
    let id1 = t.create("Open ticket");
    let id2 = t.create("Closed ticket");
    t.run_ok(&["close", &id2]);

    let out = t.run_ok(&["query", r#".status == "open""#]);
    assert!(out.contains(&id1));
    assert!(!out.contains(&id2), "Closed ticket should not appear in open query");
}

// ---------------------------------------------------------------------------
// Edit test
// ---------------------------------------------------------------------------

#[test]
fn test_edit_shows_path() {
    let t = TicketTest::new();
    let id = t.create("Test");
    let out = t.run_ok(&["edit", &id]);
    assert!(out.contains(".tickets"));
    assert!(out.contains(&format!("{}.md", id)));
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_no_tickets_directory_for_read_command() {
    let t = TicketTest::new();
    let err = t.run_fail(&["show", "nonexistent"]);
    assert!(err.contains("no .tickets directory found") || err.contains("not found"));
}

#[test]
fn test_ambiguous_id() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "First"]);
    let id2 = t.run_ok(&["create", "Second"]);

    // Extract prefixes (same for both since same dir)
    let prefix = id1.split('-').next().unwrap();

    let err = t.run_fail(&["show", prefix]);
    assert!(err.contains("ambiguous") || err.contains("matches multiple"),
        "Expected ambiguous error, got: {}", err);
}

#[test]
fn test_help_command_works_without_tickets() {
    // Run help from a directory without .tickets
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let out = tk_stdout(dir.path(), &["--help"]);
    assert!(out.contains("Create"), "Help should show available commands");
}

// ---------------------------------------------------------------------------
// Dep cycle tests
// ---------------------------------------------------------------------------

#[test]
fn test_dep_cycle_none() {
    let t = TicketTest::new();
    let id1 = t.create("A");
    let id2 = t.create("B");
    t.run_ok(&["dep", &id1, &id2]);

    let out = t.run_ok(&["dep-cycle"]);
    assert!(out.contains("No dependency cycles found"));
}

// ---------------------------------------------------------------------------
// Multi-dir parent walking
// ---------------------------------------------------------------------------

#[test]
fn test_storage_dir_walking() {
    // Create a temp dir with .tickets
    let dir = tempfile::tempdir().unwrap();
    let tickets_dir = dir.path().join(".tickets");
    fs::create_dir(&tickets_dir).unwrap();

    // Create a ticket file directly
    let id = "test-0001";
    fs::write(
        tickets_dir.join(format!("{}.md", id)),
        format!(
            "---\nid: {}\nstatus: open\ndeps: []\nlinks: []\ncreated: 2024-01-15T10:00:00Z\ntype: task\npriority: 2\n---\n\n# Test\n",
            id
        ),
    ).unwrap();

    // Test that tk show works from subdirectory
    let subdir = dir.path().join("src").join("components");
    fs::create_dir_all(&subdir).unwrap();

    let out = tk_stdout(&subdir, &["show", &id]);
    assert!(out.contains(&id));
    assert!(out.contains("Test"));
}

// ---------------------------------------------------------------------------
// Parent tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_with_parent() {
    let t = TicketTest::new();
    let parent_id = t.create("Parent epic");
    let child_id = t.run_ok(&["create", "Child task", "--parent", &parent_id]);
    let child_content = t.read_ticket_file(&child_id);
    assert!(child_content.contains(&parent_id), "Child should reference parent");
    assert!(
        child_content.contains("parent: ") || child_content.contains(&format!("parent: {}", parent_id)),
        "Child should have parent field: {}",
        child_content
    );
}

// ---------------------------------------------------------------------------
// Dep tree tests
// ---------------------------------------------------------------------------

#[test]
fn test_dep_tree_simple() {
    let t = TicketTest::new();
    let root = t.create("Root");
    let child = t.create("Child");
    t.run_ok(&["dep", &root, &child]);

    let out = t.run_ok(&["dep-tree", &root]);
    assert!(out.contains(&root), "Root ID should appear in dep-tree output");
    assert!(out.contains("Child"), "Child title should appear in dep-tree output");
    assert!(out.contains("Root"), "Root title should appear in dep-tree output");

    assert!(
        out.contains("├──") || out.contains("└──"),
        "Should have box-drawing: {}",
        out
    );
}

#[test]
fn test_dep_tree_full() {
    let t = TicketTest::new();
    let root = t.create("Root");
    let child1 = t.create("Child1");
    let child2 = t.create("Child2");
    // root depends on both
    t.run_ok(&["dep", &root, &child1]);
    t.run_ok(&["dep", &root, &child2]);
    // child1 also depends on child2 (shared dep)
    t.run_ok(&["dep", &child1, &child2]);

    let out = t.run_ok(&["dep-tree", &root, "--full"]);
    assert!(out.contains(&root), "Root ID should appear");
    assert!(out.contains("Child1"), "Child1 title should appear");
    assert!(out.contains("Child2"), "Child2 title should appear");
    assert!(out.contains("├──") || out.contains("└──"), "Should have box-drawing characters");

}

#[test]
fn test_dep_tree_non_existent() {
    let t = TicketTest::new();
    let err = t.run_fail(&["dep-tree", "nonexistent"]);
    assert!(
        err.contains("not found") || err.contains("Error"),
        "Expected error for non-existent ID: {}",
        err
    );
}

// ---------------------------------------------------------------------------
// Dep cycle detection (with real cycle)
// ---------------------------------------------------------------------------

#[test]
fn test_dep_cycle_detected() {
    let t = TicketTest::new();
    let a = t.create("A");
    let b = t.create("B");
    let c = t.create("C");
    t.run_ok(&["dep", &a, &b]);
    t.run_ok(&["dep", &b, &c]);
    t.run_ok(&["dep", &c, &a]); // A <- B <- C <- A forms a cycle

    let out = t.run_ok(&["dep-cycle"]);
    assert!(
        !out.contains("No dependency cycles found"),
        "Should have detected a cycle"
    );
    assert!(
        out.contains("->") || out.contains("cycle") || out.contains("Cycle"),
        "Should describe the cycle: {}",
        out
    );
}

#[test]
fn test_dep_cycle_with_closed_excluded() {
    let t = TicketTest::new();
    let c = t.create("C");
    let d = t.create("D");
    t.run_ok(&["dep", &c, &d]);
    t.run_ok(&["dep", &d, &c]); // cycle between C and D
    t.run_ok(&["close", &c]); // C is closed, so cycle should be ignored

    let out = t.run_ok(&["dep-cycle"]);
    assert!(
        out.contains("No dependency cycles found"),
        "Cycle with closed ticket should be ignored: {}",
        out
    );
}

// ---------------------------------------------------------------------------
// Ready / Blocked / Closed filter tests
// ---------------------------------------------------------------------------

#[test]
fn test_ready_with_assignee_filter() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "Alice ticket", "-a", "Alice"]);
    let id2 = t.run_ok(&["create", "Bob ticket", "-a", "Bob"]);

    let out = t.run_ok(&["ready", "-a", "Alice"]);
    assert!(out.contains(&id1), "Alice's ticket should be ready: {}", out);
    assert!(!out.contains(&id2), "Bob's ticket should not appear: {}", out);
}

#[test]
fn test_ready_with_tag_filter() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "UI ticket", "--tags", "ui"]);
    let id2 = t.run_ok(&["create", "Backend ticket", "--tags", "backend"]);

    let out = t.run_ok(&["ready", "-T", "ui"]);
    assert!(out.contains(&id1), "UI ticket should appear: {}", out);
    assert!(!out.contains(&id2), "Backend ticket should not appear: {}", out);
}

#[test]
fn test_blocked_with_assignee_filter() {
    let t = TicketTest::new();
    let blockee = t.run_ok(&["create", "Alice blocked", "-a", "Alice"]);
    let blocker = t.run_ok(&["create", "Blocker"]);
    t.run_ok(&["dep", &blockee, &blocker]);
    // Another blocked ticket for Bob that shouldn't appear
    let bob_blockee = t.run_ok(&["create", "Bob blocked", "-a", "Bob"]);
    let bob_blocker = t.run_ok(&["create", "Bob blocker"]);
    t.run_ok(&["dep", &bob_blockee, &bob_blocker]);

    let out = t.run_ok(&["blocked", "-a", "Alice"]);
    assert!(out.contains(&blockee), "Alice blocked should appear: {}", out);
    assert!(!out.contains(&bob_blockee), "Bob blocked should not appear: {}", out);
}

#[test]
fn test_blocked_with_tag_filter() {
    let t = TicketTest::new();
    let ui_blocked = t.run_ok(&["create", "UI blocked", "--tags", "ui"]);
    let blocker = t.create("Blocker");
    t.run_ok(&["dep", &ui_blocked, &blocker]);
    let backend_blocked = t.run_ok(&["create", "Backend blocked", "--tags", "backend"]);
    let backend_blocker = t.create("Backend blocker");
    t.run_ok(&["dep", &backend_blocked, &backend_blocker]);

    let out = t.run_ok(&["blocked", "-T", "ui"]);
    assert!(out.contains(&ui_blocked), "UI blocked should appear: {}", out);
    assert!(!out.contains(&backend_blocked), "Backend blocked should not appear: {}", out);
}

#[test]
fn test_closed_with_limit() {
    let t = TicketTest::new();
    let id1 = t.create("First");
    let id2 = t.create("Second");
    let id3 = t.create("Third");
    t.run_ok(&["close", &id1]);
    t.run_ok(&["close", &id2]);
    t.run_ok(&["close", &id3]);

    let out = t.run_ok(&["closed", "--limit", "2"]);
    // Should show at most 2 tickets
    let ticket_count = out.lines().filter(|l| l.contains('-')).count();
    assert!(
        ticket_count <= 2,
        "Expected at most 2 closed tickets, got {}: {}",
        ticket_count,
        out
    );
}

#[test]
fn test_closed_with_assignee_filter() {
    let t = TicketTest::new();
    let id_alice = t.run_ok(&["create", "Alice ticket", "-a", "Alice"]);
    let id_bob = t.run_ok(&["create", "Bob ticket", "-a", "Bob"]);
    t.run_ok(&["close", &id_alice]);
    t.run_ok(&["close", &id_bob]);

    let out = t.run_ok(&["closed", "-a", "Alice"]);
    assert!(out.contains(&id_alice), "Alice's closed ticket should appear: {}", out);
    assert!(!out.contains(&id_bob), "Bob's closed ticket should not appear: {}", out);
}

#[test]
fn test_closed_with_tag_filter() {
    let t = TicketTest::new();
    let id_ui = t.run_ok(&["create", "UI ticket", "--tags", "ui"]);
    let id_backend = t.run_ok(&["create", "Backend ticket", "--tags", "backend"]);
    t.run_ok(&["close", &id_ui]);
    t.run_ok(&["close", &id_backend]);

    let out = t.run_ok(&["closed", "-T", "ui"]);
    assert!(out.contains(&id_ui), "UI closed ticket should appear: {}", out);
    assert!(!out.contains(&id_backend), "Backend closed should not appear: {}", out);
}

// ---------------------------------------------------------------------------
// List filter tests
// ---------------------------------------------------------------------------

#[test]
fn test_list_with_assignee_filter() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "Alice ticket", "-a", "Alice"]);
    let id2 = t.run_ok(&["create", "Bob ticket", "-a", "Bob"]);

    let out = t.run_ok(&["ls", "-a", "Alice"]);
    assert!(out.contains(&id1), "Alice's ticket should appear: {}", out);
    assert!(!out.contains(&id2), "Bob's ticket should not appear: {}", out);
}

#[test]
fn test_list_with_type_filter() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "Bug ticket", "-t", "bug"]);
    let id2 = t.run_ok(&["create", "Feature ticket", "-t", "feature"]);

    let out = t.run_ok(&["ls", "-T", "bug"]);
    assert!(out.contains(&id1), "Bug ticket should appear: {}", out);
    assert!(!out.contains(&id2), "Feature ticket should not appear: {}", out);
}

#[test]
fn test_list_with_tags_filter() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "UI ticket", "--tags", "ui"]);
    let id2 = t.run_ok(&["create", "Backend ticket", "--tags", "backend"]);

    let out = t.run_ok(&["ls", "--tags", "ui"]);
    assert!(out.contains(&id1), "UI ticket should appear: {}", out);
    assert!(!out.contains(&id2), "Backend ticket should not appear: {}", out);
}

#[test]
fn test_list_with_search_filter() {
    let t = TicketTest::new();
    let id1 = t.run_ok(&["create", "Payment feature"]);
    let id2 = t.run_ok(&["create", "Login page"]);

    let out = t.run_ok(&["ls", "-s", "Payment"]);
    assert!(out.contains(&id1), "Payment ticket should appear: {}", out);
    assert!(!out.contains(&id2), "Login ticket should not appear: {}", out);
}