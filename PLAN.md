## Recommendations for `ticket-rs`

### File Format (preserve human-browsability)

The spec's format is the key insight: flat markdown files with YAML frontmatter in `.tickets/`. This means:
- **Humans** can `ls .tickets`, `cat .tickets/nw-5c46.md`, grep across files
- **IDEs** Cmd+click on ticket IDs in git log messages
- **AI agents** search via `rg` naturally
- **Rust** reads/writes each file independently — no locking, no DB migration

**Keep exactly this format.** Don't invent a new one.

---

### Data Model (directly from ticket.ts)

| Rust Struct | Fields |
|---|---|
| `Ticket` | `metadata: Metadata`, `data: Data`, `filename: PathBuf` |
| `Metadata` | `id, status, deps, links, created, type, priority, assignee?, external_ref?, parent?, tags?` |
| `Data` | `title, description?, design?, acceptance?, notes: Vec<Note>` |
| `Note` | `timestamp, body` |

---

### Recommended Architecture

```
src/
├── main.rs           # CLI entry point, clap dispatch
├── ticket.rs         # Ticket, Metadata, Data, Note structs + serde
├── id.rs             # ID generation, partial ID resolution
├── storage.rs        # .tickets/ directory walking, file I/O, CRUD
├── query.rs          # Search, filter, dedup engine
├── graph.rs          # Dep tree rendering, cycle detection, inverse relations
└── display.rs        # Human output (tables, tree, show)
```

---

### "Killer Features" for Read

You mentioned search, filter, dedup, and fetch-by-id. Here's the Rust difference-maker:

**1. Partial ID resolution** — the bash script uses `find *${id}*.md`. In Rust:
- Build an in-memory index on init (lazy, on first query)
- Match by: exact → suffix match → prefix match → substring
- Error on ambiguity (same behavior)
- O(1) lookups via `HashMap<&str, PathBuf>` once indexed

**2. Bulk query engine** — the bash script re-parses every file per command with awk. In Rust:
- Load all tickets into `Vec<Ticket>` once per command
- Chain combinators: `.filter()` on status/assignee/type/tags, `.sort()` by priority→id
- **Dedup**: for dependency trees, track visited IDs in a `HashSet` instead of awk's hacky string manipulation
- **Blocked/Ready**: compute on the in-memory graph — O(n) flat scan, no file thrashing

**3. Rich search** — far beyond `tk query`:
- Full-text search across title/description/design/notes via the `regex` crate
- Tag intersection (tags=ui,urgent → must match both) vs union
- Date range filters on `created`
- Combinable: `tk ls --status=open --assignee=trent --search "SSE" --tags urgent`

**4. Dependency graph as first-class object**:
- `Graph { adjacency: HashMap<Id, Vec<Id>> }` built from all `deps` fields
- Cycle detection via topological sort (DFS with state: white/gray/black)
- Inverse graph computed once: `blocked_by`, `children`, `blocking` for `tk show`
- Subtree depth sorting for `dep tree` — trivial with recursion in Rust vs awk's hand-rolled stack

---

### CLI Command Set (what to build)

| Priority | Command | Notes |
|---|---|---|
| P0 | `create` | Same args as tk |
| P0 | `show <id>` | Enhanced with resolved parent titles, blockers/blocking/children/linked sections (all computed from full graph) |
| P0 | `list` / `ls` | Rich filters: `--status`, `--assignee`, `--type`, `--tags`, `--search`, `--sort` |
| P0 | `status <id> <s>` | start/close/reopen aliases |
| P0 | `dep <id> <dep>` / `undep` | Add/remove dependencies |
| P0 | `link <ids...>` / `unlink` | Symmetric linking |
| P0 | `ready` / `blocked` / `closed` | Same semantics |
| P1 | `add-note <id> [text]` | Timestamped notes |
| P1 | `dep tree [--full] <id>` | Box-drawing tree with dedup |
| P1 | `dep cycle` | Find cycles |
| P2 | `query [jq-filter]` | JSONL output (for CLIs/piping) |
| P2 | `edit <id>` | `$EDITOR` launch |

---

### CRUD Storage Layer

```rust
pub struct Storage {
    dir: PathBuf,
    // In-memory index: full Id -> PathBuf (lazily populated)
    index: RefCell<HashMap<String, PathBuf>>,
}

impl Storage {
    // Finds .tickets/ by walking parents (or TICKETS_DIR env var)
    pub fn new() -> Result<Self>;

    // Resolve partial ID to full ID, error on ambiguous/not-found
    pub fn resolve_id(&self, partial: &str) -> Result<String>;

    // CRUD
    pub fn create(&self, opts: CreateOptions) -> Result<String>;  // returns id
    pub fn read(&self, id: &str) -> Result<Ticket>;
    pub fn update(&self, id: &str, field: &str, value: &str) -> Result<()>;
    pub fn delete(&self, id: &str) -> Result<()>;

    // Bulk
    pub fn all(&self) -> Result<Vec<Ticket>>;
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<Ticket>>;
}
```

**File mutation** should be safe: write to `.tickets/<id>.md.tmp`, then `rename()`. Atomic on same filesystem.

---

### What to Skip from tk

| Feature | Reason |
|---|---|
| Plugin system | Additive complexity, not core. Rust binary is self-contained. Can add plugin loading later via dlopen or subprocess. |
| `super` command | Only needed because bash plugins intercept dispatch. Not needed. |
| Homebrew/AUR packaging | Not relevant to Rust crate. Ship as `cargo install ticket-rs` + GitHub releases with prebuilt binaries. |

---

### Recommended Crate Dependencies

| Crate | Why |
|---|---|
| `clap` (with derive) | CLI arg parsing |
| `serde` + `serde_yaml` | YAML frontmatter |
| `regex` | Full-text search |
| `chrono` | Timestamps, date filtering |
| `thiserror` / `anyhow` | Error handling |
| `once_cell` or `std::sync::OnceLock` | Lazy init of ticket index |

---

### First Build Order

1. **`Storage::new()` + parent-walking** — get `find_tickets_dir` working
2. **`Ticket::parse(file)`** — YAML frontmatter + markdown body parsing
3. **`Storage::create()` + `Storage::read()` + `id::generate()`** — create a ticket, read it back
4. **`cmd_show()`** — wire up, verify full cross-referenced output
5. **`cmd_list()` + `cmd_ready()`/`cmd_blocked()`** — bulk query with filters
6. **`cmd_dep()` / `cmd_link()`** — mutate deps/links
7. **`dep tree` + `dep cycle`** — graph algorithms
8. **`add-note`** — append to body
