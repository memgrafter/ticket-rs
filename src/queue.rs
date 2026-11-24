//! Networked ticket queue: an upsert mirror of tickets plus a change log.
//!
//! `tk queue serve` keeps the queue in an append-only JSONL file and answers
//! HTTP on 127.0.0.1 only; other hosts reach it through an ssh tunnel.
//! Every line of the file is one change: the full entry after the change,
//! with a `seq` that orders all changes. The current entries (one per
//! repo + id) are rebuilt by replaying the file at start.
//!
//! Entries carry a free-form queue name (default `default`); the server keeps
//! per-queue indexes so a filtered read touches only that queue's entries.
//! Raises may carry a `group` (Sentry-style fingerprint): the first raise of
//! a (queue, repo, group) makes the entry and later ones count as occurrences
//! of it.
//!
//! Clients find the server through `TK_QUEUE_URL`. Without it, tk never
//! touches the network.

use crate::http;
use crate::storage::{Storage, Ticket};
use crate::types::{CreateOptions, Priority};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 7878;
const URL_ENV: &str = "TK_QUEUE_URL";
const MAX_REQUEST_BODY: usize = 1024 * 1024;
const SERVER_IO_TIMEOUT: Duration = Duration::from_secs(5);
const EVENTS_PAGE_LIMIT: usize = 1000;
const RAISE_ID_PREFIX: &str = "q-";
const RAISE_REF_PREFIX: &str = "queue:";
pub const DEFAULT_QUEUE: &str = "default";
const DATA_FILE_NAME: &str = "_global_queue.jsond";
/// Longest piece of a raise's description copied into an occurrence note.
const NOTE_EVIDENCE_CHARS: usize = 500;

/// The queue URL from `TK_QUEUE_URL`, if set and non-empty.
pub fn configured_url() -> Option<String> {
    std::env::var(URL_ENV)
        .ok()
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
}

fn state_dir() -> PathBuf {
    if let Ok(d) = std::env::var("TK_QUEUE_STATE_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".local/state/tk")
}

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ---------------------------------------------------------------- server

type EntryKey = (String, String);

struct QueueState {
    log: File,
    /// (repo, id) -> latest entry.
    entries: HashMap<EntryKey, Value>,
    /// queue name -> keys of the entries now in that queue.
    by_queue: HashMap<String, HashSet<EntryKey>>,
    /// (queue, repo, group) -> id of the group's raise entry.
    groups: HashMap<(String, String, String), String>,
    /// Every change, in seq order.
    events: Vec<Value>,
    /// queue name -> positions in `events` of that queue's changes.
    queue_events: HashMap<String, Vec<usize>>,
    seq: u64,
}

/// Queue name of an entry; lines written before named queues are `default`.
fn entry_queue(v: &Value) -> &str {
    match str_field(v, "queue") {
        "" => DEFAULT_QUEUE,
        q => q,
    }
}

fn entry_key(v: &Value) -> EntryKey {
    (
        str_field(v, "repo").to_string(),
        str_field(v, "id").to_string(),
    )
}

fn str_field<'a>(v: &'a Value, name: &str) -> &'a str {
    v.get(name).and_then(Value::as_str).unwrap_or("")
}

impl QueueState {
    fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let log = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)
            .with_context(|| format!("cannot open queue log {}", path.display()))?;
        log.try_lock().map_err(|_| {
            anyhow!(
                "queue log {} is in use by another tk queue serve",
                path.display()
            )
        })?;

        let mut state = QueueState {
            log,
            entries: HashMap::new(),
            by_queue: HashMap::new(),
            groups: HashMap::new(),
            events: Vec::new(),
            queue_events: HashMap::new(),
            seq: 0,
        };
        let lines: Vec<String> = BufReader::new(&state.log)
            .lines()
            .collect::<std::io::Result<_>>()?;
        for (n, line) in lines.into_iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(v) if v.get("seq").and_then(Value::as_u64).is_some() => state.apply(v),
                _ => eprintln!("warning: skipping bad line {} in {}", n + 1, path.display()),
            }
        }
        Ok(state)
    }

    fn apply(&mut self, v: Value) {
        self.seq = self.seq.max(v["seq"].as_u64().unwrap_or(0));
        let key = entry_key(&v);
        let queue = entry_queue(&v).to_string();
        if let Some(old) = self.entries.get(&key) {
            let old_queue = entry_queue(old);
            if old_queue != queue {
                if let Some(keys) = self.by_queue.get_mut(old_queue) {
                    keys.remove(&key);
                }
            }
        }
        self.by_queue
            .entry(queue.clone())
            .or_default()
            .insert(key.clone());
        let group = str_field(&v, "group");
        if str_field(&v, "kind") == "raise" && !group.is_empty() {
            self.groups.insert(
                (queue.clone(), key.0.clone(), group.to_string()),
                key.1.clone(),
            );
        }
        self.queue_events
            .entry(queue)
            .or_default()
            .push(self.events.len());
        self.entries.insert(key, v.clone());
        self.events.push(v);
    }

    /// Stamp `entry` with the next seq and time, append it durably, then apply it.
    fn record(&mut self, mut entry: Map<String, Value>) -> Result<Value> {
        entry.insert("seq".into(), json!(self.seq + 1));
        entry.insert("at".into(), json!(now()));
        let v = Value::Object(entry);
        writeln!(self.log, "{}", v)?;
        self.log.sync_data()?;
        self.apply(v.clone());
        Ok(v)
    }

    fn get(&self, repo: &str, id: &str) -> Option<&Value> {
        self.entries.get(&(repo.to_string(), id.to_string()))
    }
}

/// `~/.tickets/_global_queue.jsond` (newline-delimited JSON). Project ticket
/// lookups skip `~/.tickets`, see `storage::home_tickets_dir`.
pub fn default_data_path() -> Result<PathBuf> {
    crate::storage::home_tickets_dir()
        .map(|d| d.join(DATA_FILE_NAME))
        .ok_or_else(|| anyhow!("HOME is not set; pass --data"))
}

/// Run the queue server on 127.0.0.1:`port` until killed.
pub fn serve(port: u16, data: Option<PathBuf>) -> Result<()> {
    let path = match data {
        Some(p) => p,
        None => default_data_path()?,
    };
    let mut state = QueueState::open(&path)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .with_context(|| format!("cannot listen on 127.0.0.1:{}", port))?;
    println!(
        "tk queue serving on http://{} (log: {}, last seq: {})",
        listener.local_addr()?,
        path.display(),
        state.seq
    );
    std::io::stdout().flush()?;

    // One request at a time: the volume is tiny and it keeps the log writes ordered.
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                if let Err(e) = handle_connection(&mut state, &stream) {
                    eprintln!("warning: request failed: {:#}", e);
                }
            }
            Err(e) => eprintln!("warning: accept failed: {}", e),
        }
    }
    Ok(())
}

struct Reply {
    status: u16,
    body: String,
    jsonl: bool,
}

impl Reply {
    fn json(status: u16, v: Value) -> Self {
        Reply {
            status,
            body: v.to_string(),
            jsonl: false,
        }
    }
    fn error(status: u16, msg: impl std::fmt::Display) -> Self {
        Self::json(status, json!({ "error": msg.to_string() }))
    }
    fn lines<'a>(values: impl Iterator<Item = &'a Value>) -> Self {
        let body = values.map(|v| v.to_string() + "\n").collect();
        Reply {
            status: 200,
            body,
            jsonl: true,
        }
    }
}

fn handle_connection(state: &mut QueueState, stream: &TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(SERVER_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(SERVER_IO_TIMEOUT))?;
    let reply = match http::read_request(stream, MAX_REQUEST_BODY) {
        Ok(req) => route(state, &req).unwrap_or_else(|e| Reply::error(400, format!("{:#}", e))),
        Err(e) => Reply::error(400, format!("{:#}", e)),
    };
    let content_type = if reply.jsonl {
        "application/x-ndjson"
    } else {
        "application/json"
    };
    http::write_response(stream, reply.status, content_type, &reply.body)
}

/// Host names a local client uses; anything else is a DNS-rebinding page.
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "::1"];

fn route(state: &mut QueueState, req: &http::Request) -> Result<Reply> {
    if !LOOPBACK_HOSTS.contains(&req.host.as_str()) {
        return Ok(Reply::error(403, "Host must be 127.0.0.1 or localhost"));
    }
    if req.method == "POST" && !req.from_client {
        return Ok(Reply::error(
            403,
            format!("POST needs the header '{}: 1'", http::CLIENT_HEADER),
        ));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => Ok(Reply::json(200, json!({ "ok": true, "seq": state.seq }))),
        ("POST", "/push") => handle_push(state, &request_params(req)?),
        ("POST", "/raise") => handle_raise(state, &request_params(req)?),
        ("POST", "/claim") => handle_claim(state, &request_params(req)?),
        ("POST", "/noted") => handle_noted(state, &request_params(req)?),
        ("GET", "/events") => handle_events(state, &req.query),
        ("GET", "/entries") => Ok(handle_entries(state, &req.query)),
        ("GET", "/entry") => handle_entry(state, &req.query),
        _ => Ok(Reply::error(
            404,
            format!("no route {} {}", req.method, req.path),
        )),
    }
}

/// Body as a JSON object, or form fields as strings (what `curl --data-urlencode` sends).
fn request_params(req: &http::Request) -> Result<Map<String, Value>> {
    if req.content_type.starts_with("application/json") {
        match serde_json::from_slice(&req.body).context("body is not valid JSON")? {
            Value::Object(m) => Ok(m),
            _ => bail!("JSON body must be an object"),
        }
    } else {
        let body = String::from_utf8_lossy(&req.body);
        Ok(http::form_decode(&body)
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect())
    }
}

fn required<'a>(params: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    match params.get(name).and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => bail!("missing required field '{}'", name),
    }
}

fn optional(params: &Map<String, Value>, name: &str) -> Option<String> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn handle_push(state: &mut QueueState, p: &Map<String, Value>) -> Result<Reply> {
    let repo = required(p, "repo")?;
    let id = required(p, "id")?;
    let ticket = p
        .get("ticket")
        .filter(|t| t.is_object())
        .ok_or_else(|| anyhow!("missing required object 'ticket'"))?;

    let raise_id = optional(p, "raise_id");
    // Explicit name, else where this ticket (or the raise it came from) already is.
    let queue = optional(p, "queue")
        .or_else(|| state.get(repo, id).map(|e| entry_queue(e).to_string()))
        .or_else(|| {
            raise_id
                .as_ref()
                .and_then(|r| state.get(repo, r))
                .map(|e| entry_queue(e).to_string())
        })
        .unwrap_or_else(|| DEFAULT_QUEUE.to_string());

    let mut entry = Map::new();
    entry.insert("queue".into(), json!(queue));
    entry.insert("repo".into(), json!(repo));
    entry.insert("id".into(), json!(id));
    entry.insert("kind".into(), json!("ticket"));
    entry.insert(
        "op".into(),
        json!(optional(p, "op").unwrap_or_else(|| "update".into())),
    );
    entry.insert(
        "host".into(),
        json!(optional(p, "host").unwrap_or_else(|| "unknown".into())),
    );
    if let Some(path) = optional(p, "path") {
        entry.insert("path".into(), json!(path));
    }
    if let Some(ref r) = raise_id {
        entry.insert("raise_id".into(), json!(r));
    }
    entry.insert("ticket".into(), ticket.clone());
    let recorded = state.record(entry)?;

    if let Some(r) = raise_id {
        if let Some(Value::Object(raise)) = state.get(repo, &r).cloned() {
            let mut raise = raise;
            raise.insert("op".into(), json!("materialize"));
            raise.insert("state".into(), json!("materialized"));
            raise.insert("ticket_id".into(), json!(id));
            state.record(raise)?;
        }
    }
    Ok(Reply::json(200, recorded))
}

fn handle_raise(state: &mut QueueState, p: &Map<String, Value>) -> Result<Reply> {
    let repo = required(p, "repo")?;
    // The title becomes the ticket's one `# title` line.
    let title = required(p, "title")?.replace(['\r', '\n'], " ");
    let priority = match p.get("priority") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(2),
        Some(Value::String(s)) if !s.is_empty() => s.parse().context("priority must be 0-4")?,
        _ => 2,
    };
    if priority > 4 {
        bail!("priority must be 0-4");
    }
    let tags: Vec<String> = match p.get("tags") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => split_tags(s),
        _ => Vec::new(),
    };

    let queue = optional(p, "queue").unwrap_or_else(|| DEFAULT_QUEUE.to_string());
    let group = optional(p, "group");
    let host = optional(p, "host").unwrap_or_else(|| "unknown".into());
    let at = now();
    let latest = json!({
        "title": title,
        "description": optional(p, "description"),
        "host": host,
        "at": at,
    });

    if let Some(ref g) = group {
        let existing = state
            .groups
            .get(&(queue.clone(), repo.to_string(), g.clone()))
            .and_then(|id| state.get(repo, id))
            .cloned();
        if let Some(Value::Object(mut entry)) = existing {
            let count = entry.get("count").and_then(Value::as_u64).unwrap_or(1) + 1;
            entry.insert("op".into(), json!("occurrence"));
            entry.insert("count".into(), json!(count));
            entry.insert("last_seen".into(), json!(at));
            entry.insert("latest".into(), latest);
            return Ok(Reply::json(200, state.record(entry)?));
        }
    }

    let id = loop {
        let candidate = format!("{}{}", RAISE_ID_PREFIX, random_suffix(8));
        if state.get(repo, &candidate).is_none() {
            break candidate;
        }
    };
    let mut entry = Map::new();
    entry.insert("queue".into(), json!(queue));
    entry.insert("repo".into(), json!(repo));
    entry.insert("id".into(), json!(id));
    entry.insert("kind".into(), json!("raise"));
    entry.insert("op".into(), json!("raise"));
    entry.insert("host".into(), json!(host));
    entry.insert("state".into(), json!("pending"));
    if let Some(g) = group {
        entry.insert("group".into(), json!(g));
    }
    entry.insert("count".into(), json!(1));
    entry.insert("first_seen".into(), json!(at));
    entry.insert("last_seen".into(), json!(at));
    entry.insert("latest".into(), latest);
    // Occurrences the ticket already shows (the first is the ticket itself).
    entry.insert("noted".into(), json!(1));
    entry.insert(
        "ticket".into(),
        json!({
            "title": title,
            "description": optional(p, "description"),
            "type": optional(p, "type").unwrap_or_else(|| "task".into()),
            "priority": priority,
            "assignee": optional(p, "assignee"),
            "tags": tags,
        }),
    );
    Ok(Reply::json(200, state.record(entry)?))
}

/// Mark a pending raise as being turned into a ticket by `host`, so two
/// pollers on the same repo don't both create it. A claim by the same host
/// is accepted again (a retry after a crash).
fn handle_claim(state: &mut QueueState, p: &Map<String, Value>) -> Result<Reply> {
    let repo = required(p, "repo")?;
    let id = required(p, "id")?;
    let host = required(p, "host")?;
    let Some(Value::Object(raise)) = state.get(repo, id).cloned() else {
        return Ok(Reply::error(
            404,
            format!("no raise {} in repo {}", id, repo),
        ));
    };
    let current = raise.get("state").and_then(Value::as_str).unwrap_or("");
    let claimed_by = raise
        .get("claimed_by")
        .and_then(Value::as_str)
        .unwrap_or("");
    match current {
        "pending" => {
            let mut raise = raise;
            raise.insert("op".into(), json!("claim"));
            raise.insert("state".into(), json!("claimed"));
            raise.insert("claimed_by".into(), json!(host));
            Ok(Reply::json(200, state.record(raise)?))
        }
        "claimed" if claimed_by == host => Ok(Reply::json(200, Value::Object(raise))),
        _ => Ok(Reply::error(
            409,
            format!("raise {} is {} (claimed by {})", id, current, claimed_by),
        )),
    }
}

/// The poller wrote occurrences up to `count` into the group's ticket.
fn handle_noted(state: &mut QueueState, p: &Map<String, Value>) -> Result<Reply> {
    let repo = required(p, "repo")?;
    let id = required(p, "id")?;
    let count = match p.get("count") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
    .ok_or_else(|| anyhow!("missing required number 'count'"))?;
    let Some(Value::Object(mut raise)) = state.get(repo, id).cloned() else {
        return Ok(Reply::error(
            404,
            format!("no raise {} in repo {}", id, repo),
        ));
    };
    let noted = raise.get("noted").and_then(Value::as_u64).unwrap_or(1);
    if count <= noted {
        return Ok(Reply::json(200, Value::Object(raise)));
    }
    raise.insert("op".into(), json!("noted"));
    raise.insert("noted".into(), json!(count));
    Ok(Reply::json(200, state.record(raise)?))
}

fn handle_events(state: &QueueState, q: &HashMap<String, String>) -> Result<Reply> {
    let since: u64 = q
        .get("since")
        .map(|s| s.parse())
        .transpose()
        .context("bad since")?
        .unwrap_or(0);
    let limit: usize = q
        .get("limit")
        .map(|s| s.parse())
        .transpose()
        .context("bad limit")?
        .unwrap_or(EVENTS_PAGE_LIMIT)
        .min(EVENTS_PAGE_LIMIT);
    let seq_of = |i: usize| state.events[i]["seq"].as_u64().unwrap_or(0);
    let Some(queue) = q.get("queue") else {
        let start = state
            .events
            .partition_point(|e| e["seq"].as_u64().unwrap_or(0) <= since);
        return Ok(Reply::lines(state.events[start..].iter().take(limit)));
    };
    let positions: &[usize] = state.queue_events.get(queue).map_or(&[], Vec::as_slice);
    let start = positions.partition_point(|&i| seq_of(i) <= since);
    Ok(Reply::lines(
        positions[start..]
            .iter()
            .take(limit)
            .map(|&i| &state.events[i]),
    ))
}

fn handle_entries(state: &QueueState, q: &HashMap<String, String>) -> Reply {
    let wanted = |e: &&Value| {
        ["repo", "kind", "state", "id", "group"]
            .iter()
            .all(|f| q.get(*f).is_none_or(|want| str_field(e, f) == want))
    };
    let mut matches: Vec<&Value> = match q.get("queue") {
        Some(name) => state
            .by_queue
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|k| state.entries.get(k))
            .filter(wanted)
            .collect(),
        None => state.entries.values().filter(wanted).collect(),
    };
    matches.sort_by_key(|e| e["seq"].as_u64());
    Reply::lines(matches.into_iter())
}

fn handle_entry(state: &QueueState, q: &HashMap<String, String>) -> Result<Reply> {
    let id = q
        .get("id")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing id"))?;
    let matches: Vec<&Value> = state
        .entries
        .values()
        .filter(|e| {
            str_field(e, "id") == id && q.get("repo").is_none_or(|r| str_field(e, "repo") == r)
        })
        .collect();
    Ok(match matches.as_slice() {
        [] => Reply::error(404, format!("no queue entry {}", id)),
        [one] => Reply::json(200, (*one).clone()),
        many => {
            let repos: Vec<&str> = many.iter().map(|e| str_field(e, "repo")).collect();
            Reply::error(
                409,
                format!(
                    "{} is in several repos ({}); pass --repo",
                    id,
                    repos.join(", ")
                ),
            )
        }
    })
}

fn random_suffix(len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..len)
        .map(|_| CHARS[fastrand::usize(..CHARS.len())] as char)
        .collect()
}

fn split_tags(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

// ---------------------------------------------------------------- client

/// Call the queue; any status other than 200 is an error carrying the server's message.
fn api(method: &str, path: &str, body: Option<&Value>) -> Result<String> {
    let url = configured_url().ok_or_else(|| anyhow!("{} is not set", URL_ENV))?;
    let body = body.map(Value::to_string);
    let (status, text) = http::call(&url, method, path, body.as_deref())?;
    if status != 200 {
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
            .unwrap_or(text);
        bail!("queue answered {}: {}", status, msg);
    }
    Ok(text)
}

fn parse_lines(text: &str) -> Result<Vec<Value>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).context("queue sent a bad JSON line"))
        .collect()
}

fn host_name() -> String {
    if let Ok(h) = std::env::var("TK_QUEUE_HOST") {
        if !h.is_empty() {
            return h;
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Repo name used in queue keys: `TK_QUEUE_REPO`, else the name of the
/// directory that holds `.tickets`.
pub fn repo_name(storage: &Storage) -> String {
    if let Ok(r) = std::env::var("TK_QUEUE_REPO") {
        if !r.is_empty() {
            return r;
        }
    }
    let dir = storage
        .dir()
        .canonicalize()
        .unwrap_or_else(|_| storage.dir().to_path_buf());
    dir.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn snapshot(t: &Ticket) -> Value {
    let notes: Vec<Value> = t
        .data
        .notes
        .iter()
        .flatten()
        .map(|n| json!({ "timestamp": n.timestamp, "body": n.body }))
        .collect();
    json!({
        "id": t.metadata.id,
        "status": t.metadata.status,
        "open": t.metadata.open,
        "deps": t.metadata.deps,
        "links": t.metadata.links,
        "created": t.metadata.created,
        "type": t.metadata.metadata_type,
        "priority": t.metadata.priority.to_u8(),
        "assignee": t.metadata.assignee,
        "externalRef": t.metadata.external_ref,
        "parent": t.metadata.parent,
        "tags": t.metadata.tags,
        "title": t.data.title,
        "description": t.data.description,
        "design": t.data.design,
        "acceptance": t.data.acceptance,
        "notes": notes,
        "body": t.body,
    })
}

/// Queue for pushed tickets: `TK_QUEUE_NAME`, else the server keeps the
/// ticket's current queue (a new ticket goes to `default`).
fn queue_name_env() -> Option<String> {
    std::env::var("TK_QUEUE_NAME")
        .ok()
        .filter(|q| !q.is_empty())
}

fn push_ticket(storage: &Storage, id: &str, op: &str, raise_id: Option<&str>) -> Result<()> {
    let ticket = storage.read(id)?;
    let body = json!({
        "queue": queue_name_env(),
        "repo": repo_name(storage),
        "id": id,
        "op": op,
        "host": host_name(),
        "path": storage.dir().canonicalize().unwrap_or_else(|_| storage.dir().to_path_buf()),
        "raise_id": raise_id,
        "ticket": snapshot(&ticket),
    });
    api("POST", "/push", Some(&body)).map(|_| ())
}

/// After a ticket change, send each changed ticket to the queue (when one is
/// configured). The ticket files are already saved, so a queue failure is
/// only a warning.
pub fn mirror(storage: &Storage, ids: &[&str], op: &str) {
    if configured_url().is_none() {
        return;
    }
    for id in ids {
        if let Err(e) = push_ticket(storage, id, op, None) {
            eprintln!(
                "warning: ticket {} is saved, but the queue update failed: {:#}",
                id, e
            );
            return;
        }
    }
}

pub struct RaiseOptions {
    pub title: String,
    pub repo: Option<String>,
    pub description: Option<String>,
    pub issue_type: Option<String>,
    pub priority: Option<u8>,
    pub assignee: Option<String>,
    pub tags: Option<String>,
    pub queue: Option<String>,
    pub group: Option<String>,
}

pub fn cmd_raise(o: RaiseOptions) -> Result<()> {
    let repo = match o.repo {
        Some(r) => r,
        None => match Storage::new(false) {
            Ok(storage) => repo_name(&storage),
            Err(_) => bail!("--repo is required outside a repo (the repo whose poller should create the ticket)"),
        },
    };
    let body = json!({
        "repo": repo,
        "title": o.title,
        "description": o.description,
        "type": o.issue_type,
        "priority": o.priority,
        "assignee": o.assignee,
        "tags": o.tags.as_deref().map(split_tags).unwrap_or_default(),
        "host": host_name(),
        "queue": o.queue,
        "group": o.group,
    });
    let entry: Value = serde_json::from_str(&api("POST", "/raise", Some(&body))?)?;
    println!("{}", str_field(&entry, "id"));
    let count = entry["count"].as_u64().unwrap_or(1);
    if count > 1 {
        eprintln!(
            "Group {} raised again: occurrence {} of {}",
            str_field(&entry, "group"),
            count,
            str_field(&entry, "id")
        );
    }
    Ok(())
}

pub fn cmd_ls(
    queue: Option<String>,
    repo: Option<String>,
    kind: Option<String>,
    as_json: bool,
) -> Result<()> {
    let mut path = "/entries?".to_string();
    if let Some(q) = queue {
        path.push_str(&format!("queue={}&", http::percent_encode(&q)));
    }
    if let Some(r) = repo {
        path.push_str(&format!("repo={}&", http::percent_encode(&r)));
    }
    if let Some(k) = kind {
        path.push_str(&format!("kind={}&", http::percent_encode(&k)));
    }
    let text = api("GET", path.trim_end_matches(['?', '&']), None)?;
    if as_json {
        print!("{}", text);
        return Ok(());
    }
    for e in parse_lines(&text)? {
        let ticket = &e["ticket"];
        let state = if str_field(&e, "kind") == "raise" {
            let mut s = format!("raise {}", str_field(&e, "state"));
            if let Some(t) = e.get("ticket_id").and_then(Value::as_str) {
                s.push_str(&format!(" -> {}", t));
            }
            if let Some(g) = e.get("group").and_then(Value::as_str) {
                s.push_str(&format!(
                    ", group {} x{}",
                    g,
                    e["count"].as_u64().unwrap_or(1)
                ));
            }
            s
        } else {
            str_field(ticket, "status").to_string()
        };
        println!(
            "{:>5}  {:<10} {}/{:<12} [{}] {}",
            e["seq"].as_u64().unwrap_or(0),
            entry_queue(&e),
            str_field(&e, "repo"),
            str_field(&e, "id"),
            state,
            str_field(ticket, "title")
        );
    }
    Ok(())
}

pub fn cmd_show(id: &str, repo: Option<String>) -> Result<()> {
    let mut path = format!("/entry?id={}", http::percent_encode(id));
    if let Some(r) = repo {
        path.push_str(&format!("&repo={}", http::percent_encode(&r)));
    }
    let entry: Value = serde_json::from_str(&api("GET", &path, None)?)?;
    println!("{}", serde_json::to_string_pretty(&entry)?);
    Ok(())
}

pub struct PollOptions {
    pub since: Option<u64>,
    pub once: bool,
    pub interval: u64,
    pub exec: Option<String>,
    pub materialize: bool,
    pub cursor: Option<PathBuf>,
    pub queue: Option<String>,
}

/// Print queue changes after the cursor as JSONL, forever or `--once`.
///
/// Inside a repo, pending raises for that repo are first turned into ticket
/// files. The cursor is saved after each printed (and, with `--exec`,
/// handled) event, so a restart continues where it stopped.
pub fn cmd_poll(o: PollOptions) -> Result<()> {
    if configured_url().is_none() {
        bail!(
            "{} is not set (e.g. http://127.0.0.1:{})",
            URL_ENV,
            DEFAULT_PORT
        );
    }
    let storage = Storage::new(false).ok();
    let cursor_path = o.cursor.clone().unwrap_or_else(|| {
        let mut name = storage
            .as_ref()
            .map(repo_name)
            .unwrap_or_else(|| "no-repo".into());
        if let Some(ref q) = o.queue {
            name.push_str(&format!("--{}", q));
        }
        state_dir().join(format!("cursor-{}", name))
    });
    let mut cursor = match o.since {
        Some(s) => s,
        None => read_cursor(&cursor_path)?,
    };
    warn_if_cursor_ahead(cursor);

    loop {
        if let (true, Some(s)) = (o.materialize, storage.as_ref()) {
            if let Err(e) = materialize_pending(s, o.queue.as_deref()) {
                eprintln!("warning: could not turn queue raises into tickets: {:#}", e);
            }
        }
        let result = poll_once(
            &mut cursor,
            &cursor_path,
            o.queue.as_deref(),
            o.exec.as_deref(),
        );
        if o.once {
            return result;
        }
        if let Err(e) = result {
            eprintln!("warning: poll failed: {:#}", e);
        }
        std::thread::sleep(Duration::from_secs(o.interval.max(1)));
    }
}

/// A cursor past the queue's last seq means the queue log was replaced;
/// polling would then print nothing until the new log catches up.
fn warn_if_cursor_ahead(cursor: u64) {
    let last = api("GET", "/health", None)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["seq"].as_u64());
    if let Some(last) = last.filter(|&l| l < cursor) {
        eprintln!(
            "warning: cursor {} is past the queue's last seq {} (was the queue log replaced?); use --since 0 to start over",
            cursor, last
        );
    }
}

fn poll_once(
    cursor: &mut u64,
    cursor_path: &Path,
    queue: Option<&str>,
    exec: Option<&str>,
) -> Result<()> {
    let queue_filter = queue
        .map(|q| format!("&queue={}", http::percent_encode(q)))
        .unwrap_or_default();
    loop {
        let path = format!("/events?since={}{}", cursor, queue_filter);
        let events = parse_lines(&api("GET", &path, None)?)?;
        let page_full = events.len() >= EVENTS_PAGE_LIMIT;
        for event in &events {
            let line = event.to_string();
            println!("{}", line);
            std::io::stdout().flush()?;
            if let Some(cmd) = exec {
                run_exec(cmd, event, &line)?;
            }
            *cursor = event["seq"].as_u64().unwrap_or(*cursor);
            write_cursor(cursor_path, *cursor)?;
        }
        if !page_full {
            return Ok(());
        }
    }
}

fn run_exec(cmd: &str, event: &Value, line: &str) -> Result<()> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .env("TK_QUEUE_SEQ", event["seq"].to_string())
        .env("TK_QUEUE_NAME", entry_queue(event))
        .env("TK_QUEUE_REPO", str_field(event, "repo"))
        .env("TK_QUEUE_ID", str_field(event, "id"))
        .env("TK_QUEUE_KIND", str_field(event, "kind"))
        .env("TK_QUEUE_OP", str_field(event, "op"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("cannot start --exec command")?;
    if let Some(mut stdin) = child.stdin.take() {
        // The command may not read stdin; a broken pipe is fine.
        let _ = writeln!(stdin, "{}", line);
    }
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "--exec command failed ({}) on seq {}; cursor stays before it",
            status,
            event["seq"]
        );
    }
    Ok(())
}

fn read_cursor(path: &Path) -> Result<u64> {
    match fs::read_to_string(path) {
        Ok(s) => s
            .trim()
            .parse()
            .with_context(|| format!("bad cursor file {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e).with_context(|| format!("cannot read cursor {}", path.display())),
    }
}

fn write_cursor(path: &Path, seq: u64) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, format!("{}\n", seq))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot write cursor {}", path.display()))
}

/// Turn this repo's pending raises into ticket files, and write new
/// occurrences of grouped raises into the tickets this host made.
fn materialize_pending(storage: &Storage, queue: Option<&str>) -> Result<()> {
    let repo = repo_name(storage);
    let host = host_name();
    let mut path = format!("/entries?repo={}&kind=raise", http::percent_encode(&repo));
    if let Some(q) = queue {
        path.push_str(&format!("&queue={}", http::percent_encode(q)));
    }
    for raise in parse_lines(&api("GET", &path, None)?)? {
        if let Err(e) = handle_raise_entry(storage, &repo, &host, &raise) {
            eprintln!("warning: queue raise {}: {:#}", str_field(&raise, "id"), e);
        }
    }
    Ok(())
}

fn handle_raise_entry(storage: &Storage, repo: &str, host: &str, raise: &Value) -> Result<()> {
    let state = str_field(raise, "state");
    let claimed_by_me = str_field(raise, "claimed_by") == host;
    let ticket_id = match state {
        "pending" => materialize_one(storage, repo, host, raise)?,
        "claimed" if claimed_by_me => materialize_one(storage, repo, host, raise)?,
        "materialized" if claimed_by_me => raise
            .get("ticket_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    };
    match ticket_id {
        Some(t) => note_occurrences(storage, repo, raise, &t),
        None => Ok(()),
    }
}

/// Append one note for the occurrences of a grouped raise that the ticket
/// does not show yet. A closed ticket is reopened (a regression): closed
/// means fixed, and `tk ready`/`ls --open` would hide a flag on a closed one.
fn note_occurrences(storage: &Storage, repo: &str, raise: &Value, ticket_id: &str) -> Result<()> {
    let count = raise["count"].as_u64().unwrap_or(1);
    let noted = raise["noted"].as_u64().unwrap_or(1);
    if count <= noted {
        return Ok(());
    }
    let raise_id = str_field(raise, "id");
    let marker = format!("{}{} occurrence {}", RAISE_REF_PREFIX, raise_id, count);
    let ticket = storage.read(ticket_id)?;
    let already_written = ticket
        .data
        .notes
        .iter()
        .flatten()
        .any(|n| n.body.contains(&marker));

    let mut op = "occurrence";
    if !already_written {
        let reopen = !ticket.metadata.open;
        if reopen {
            storage.update_field(ticket_id, "open", "true")?;
            storage.update_field(ticket_id, "status", "open")?;
            op = "regression";
        }
        storage.add_note(
            ticket_id,
            &occurrence_note(raise, count - noted, count, &marker, reopen),
        )?;
        eprintln!(
            "{} {}: group raised again ({} in all)",
            if reopen { "Reopened" } else { "Noted" },
            ticket_id,
            count
        );
    }
    push_ticket(storage, ticket_id, op, None)?;
    api(
        "POST",
        "/noted",
        Some(&json!({ "repo": repo, "id": raise_id, "count": count })),
    )
    .map(|_| ())
}

fn occurrence_note(raise: &Value, new: u64, count: u64, marker: &str, reopened: bool) -> String {
    let latest = &raise["latest"];
    let mut note = String::new();
    if reopened {
        note.push_str(
            "Regression: this ticket was closed and its group raised again, so it is reopened.\n\n",
        );
    }
    note.push_str(&format!(
        "Raised again {} time(s), {} in all ({}). Latest from host {} at {}: {}",
        new,
        count,
        marker,
        str_field(latest, "host"),
        str_field(latest, "at"),
        str_field(latest, "title")
    ));
    let evidence = str_field(latest, "description");
    if !evidence.is_empty() {
        let mut short: String = evidence.chars().take(NOTE_EVIDENCE_CHARS).collect();
        if short.len() < evidence.len() {
            short.push_str(" [...]");
        }
        note.push_str("\n\n");
        note.push_str(&short);
    }
    note
}

/// Claim a raise and create its ticket; returns the ticket id, or None when
/// another poller holds the claim.
fn materialize_one(
    storage: &Storage,
    repo: &str,
    host: &str,
    raise: &Value,
) -> Result<Option<String>> {
    let raise_id = str_field(raise, "id");
    let claim = json!({ "repo": repo, "id": raise_id, "host": host });
    if let Err(e) = api("POST", "/claim", Some(&claim)) {
        eprintln!("note: skipping raise {}: {:#}", raise_id, e);
        return Ok(None);
    }

    let external_ref = format!("{}{}", RAISE_REF_PREFIX, raise_id);
    let existing = storage
        .all()?
        .into_iter()
        .find(|t| t.metadata.external_ref.as_deref() == Some(external_ref.as_str()));
    let ticket_id = match existing {
        Some(t) => t.metadata.id,
        None => create_from_raise(storage, raise, external_ref)?,
    };
    push_ticket(storage, &ticket_id, "create", Some(raise_id))?;
    eprintln!("Created {} from queue raise {}", ticket_id, raise_id);
    Ok(Some(ticket_id))
}

fn create_from_raise(storage: &Storage, raise: &Value, external_ref: String) -> Result<String> {
    let t = &raise["ticket"];
    let text = |name: &str| t.get(name).and_then(Value::as_str).map(str::to_string);
    let tags: Vec<String> = t["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let opts = CreateOptions {
        title: text("title"),
        description: text("description"),
        design: None,
        acceptance: None,
        create_type: text("type"),
        priority: t["priority"]
            .as_u64()
            .and_then(|p| Priority::from_u8(p as u8)),
        assignee: text("assignee"),
        external_ref: Some(external_ref),
        parent: None,
        tags: if tags.is_empty() { None } else { Some(tags) },
    };
    let id = storage.create(&opts)?;
    storage.add_note(
        &id,
        &format!(
            "Raised through the tk queue as {} from host {} at {}.",
            str_field(raise, "id"),
            str_field(raise, "host"),
            str_field(raise, "at")
        ),
    )?;
    Ok(id)
}
