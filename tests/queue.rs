//! Integration tests for `tk queue`: a real `tk queue serve` on 127.0.0.1
//! (any free port) and tk clients in temporary repos.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tk"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

/// Read one line from a child's stdout, giving up after `WAIT`.
fn spawn_line_reader(stdout: std::process::ChildStdout) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// A `tk queue serve` child process, killed on drop.
struct Server {
    child: Child,
    url: String,
}

impl Server {
    fn start(data: &Path) -> Self {
        let mut child = Command::new(bin())
            .args(["queue", "serve", "--port", "0", "--data"])
            .arg(data)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start tk queue serve");
        let lines = spawn_line_reader(child.stdout.take().unwrap());
        let first = match lines.recv_timeout(WAIT) {
            Ok(line) => line,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("tk queue serve did not report its address");
            }
        };
        let url = first
            .split_whitespace()
            .find(|w| w.starts_with("http://"))
            .unwrap_or_else(|| panic!("no URL in {:?}", first))
            .to_string();
        Server { child, url }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One host running tk in its own directory (a repo named `name`, or no repo).
struct Client {
    _root: tempfile::TempDir,
    dir: PathBuf,
    state: PathBuf,
    url: String,
    host: String,
}

impl Client {
    fn new(name: &str, url: &str, host: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(name);
        let state = root.path().join("state");
        std::fs::create_dir_all(&dir).unwrap();
        Client {
            _root: root,
            dir,
            state,
            url: url.to_string(),
            host: host.to_string(),
        }
    }

    /// Give this client an empty `.tickets` dir, like a repo with no tickets yet.
    fn init_repo(self) -> Self {
        std::fs::create_dir_all(self.dir.join(".tickets")).unwrap();
        self
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(bin());
        c.args(args)
            .current_dir(&self.dir)
            .env("TK_QUEUE_URL", &self.url)
            .env("TK_QUEUE_STATE_DIR", &self.state)
            .env("TK_QUEUE_HOST", &self.host)
            .env_remove("TK_QUEUE_REPO")
            .env_remove("TICKETS_DIR");
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run tk")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "tk {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            text(&out.stdout),
            text(&out.stderr)
        );
        text(&out.stdout)
    }

    fn entry(&self, id: &str) -> Value {
        serde_json::from_str(&self.ok(&["queue", "show", id])).unwrap()
    }

    fn poll_once(&self, extra: &[&str]) -> Vec<Value> {
        let mut args = vec!["queue", "poll", "--once"];
        args.extend_from_slice(extra);
        jsonl(&self.ok(&args))
    }

    fn ticket_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir.join(".tickets"))
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn jsonl(s: &str) -> Vec<Value> {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad JSON line {:?}: {}", l, e)))
        .collect()
}

fn seqs(events: &[Value]) -> Vec<u64> {
    events.iter().map(|e| e["seq"].as_u64().unwrap()).collect()
}

fn data_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// POST a form body the way `curl --data-urlencode` does (the vert batch script path).
fn post_form(url: &str, path: &str, body: &str) -> (u16, String) {
    let authority = url.strip_prefix("http://").unwrap();
    let mut s = TcpStream::connect(authority).unwrap();
    s.set_read_timeout(Some(WAIT)).unwrap();
    write!(
        s,
        "POST {} HTTP/1.1\r\nHost: {}\r\nX-TK-Queue: 1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        authority,
        body.len(),
        body
    )
    .unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, body.to_string())
}

#[test]
fn test_queue_create_pushes_entry() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let mac = Client::new("alpha", &server.url, "mac");

    let id = mac.ok(&[
        "create",
        "Disk full",
        "-t",
        "bug",
        "-p",
        "1",
        "--tags",
        "ops",
    ]);
    let e = mac.entry(&id);
    assert_eq!(e["repo"], "alpha");
    assert_eq!(e["id"], id.as_str());
    assert_eq!(e["kind"], "ticket");
    assert_eq!(e["op"], "create");
    assert_eq!(e["host"], "mac");
    assert_eq!(e["seq"], 1);
    assert_eq!(e["ticket"]["title"], "Disk full");
    assert_eq!(e["ticket"]["type"], "bug");
    assert_eq!(e["ticket"]["priority"], 1);
    assert_eq!(e["ticket"]["tags"][0], "ops");
}

#[test]
fn test_queue_ticket_changes_update_the_same_entry() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let mac = Client::new("alpha", &server.url, "mac");
    let a = mac.ok(&["create", "A"]);
    let b = mac.ok(&["create", "B"]);

    mac.ok(&["start", &a]);
    assert_eq!(mac.entry(&a)["ticket"]["status"], "in_progress");
    mac.ok(&["add-note", &a, "found it"]);
    assert_eq!(mac.entry(&a)["ticket"]["notes"][0]["body"], "found it");
    mac.ok(&["dep", &a, &b]);
    assert_eq!(mac.entry(&a)["ticket"]["deps"][0], b.as_str());
    mac.ok(&["link", &a, &b]);
    assert_eq!(mac.entry(&a)["ticket"]["links"][0], b.as_str());
    assert_eq!(mac.entry(&b)["ticket"]["links"][0], a.as_str());
    mac.ok(&["undep", &a, &b]);
    mac.ok(&["unlink", &a, &b]);
    mac.ok(&["status", &b, "waiting"]);
    mac.ok(&["close", &a]);
    mac.ok(&["reopen", &b]);

    let ea = mac.entry(&a);
    assert_eq!(ea["ticket"]["status"], "closed");
    assert_eq!(ea["ticket"]["open"], false);
    assert_eq!(ea["ticket"]["deps"], Value::Array(vec![]));
    assert_eq!(ea["ticket"]["links"], Value::Array(vec![]));
    assert_eq!(mac.entry(&b)["ticket"]["status"], "open");

    // Upsert: still one entry per ticket, but every change is in the log.
    let entries = jsonl(&mac.ok(&["queue", "ls", "--json"]));
    assert_eq!(entries.len(), 2);
    let ops: Vec<String> = mac
        .poll_once(&["--since", "0"])
        .iter()
        .filter(|e| e["id"] == a.as_str())
        .map(|e| e["op"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        ops,
        ["create", "status", "note", "dep", "link", "undep", "unlink", "status"]
    );
}

#[test]
fn test_queue_raise_from_host_without_repo_becomes_ticket() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let vert = Client::new("batch", &server.url, "vert");
    let mac = Client::new("alpha", &server.url, "mac").init_repo();
    assert!(!vert.dir.join(".tickets").exists());

    // tk on a host with no .tickets, and plain curl-style form POST.
    let r1 = vert.ok(&[
        "queue",
        "raise",
        "Mesh export failed",
        "--repo",
        "alpha",
        "-d",
        "exit 3",
        "-p",
        "1",
        "--tags",
        "vert,batch",
    ]);
    let (status, body) = post_form(
        &server.url,
        "/raise",
        "repo=alpha&title=GPU+out%0Aof+memory&description=line1%0Aline2&host=vert&type=bug",
    );
    assert_eq!(status, 200, "{}", body);
    let r2 = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = post_form(&server.url, "/raise", "repo=other&title=Not+for+alpha");
    assert_eq!(status, 200);
    let (status, body) = post_form(&server.url, "/raise", "repo=alpha");
    assert_eq!(status, 400, "raise without a title must fail: {}", body);
    assert!(
        !vert.dir.join(".tickets").exists(),
        "raise must not create .tickets"
    );
    assert_eq!(vert.entry(&r1)["state"], "pending");

    mac.poll_once(&[]);
    let files = mac.ticket_files();
    assert_eq!(files.len(), 2, "{:?}", files);

    let e1 = mac.entry(&r1);
    assert_eq!(e1["state"], "materialized");
    let t1 = e1["ticket_id"].as_str().unwrap().to_string();
    let file =
        std::fs::read_to_string(mac.dir.join(".tickets").join(format!("{}.md", t1))).unwrap();
    assert!(file.contains("# Mesh export failed"), "{}", file);
    assert!(
        file.contains(&format!("external-ref: queue:{}", r1)),
        "{}",
        file
    );
    assert!(file.contains("priority: 1"), "{}", file);
    assert!(file.contains("tags: [vert, batch]"), "{}", file);
    assert!(file.contains("from host vert"), "{}", file);
    assert_eq!(mac.entry(&t1)["raise_id"], r1.as_str());

    let t2 = mac.entry(&r2)["ticket_id"].as_str().unwrap().to_string();
    let file2 =
        std::fs::read_to_string(mac.dir.join(".tickets").join(format!("{}.md", t2))).unwrap();
    assert!(file2.contains("line1\nline2"), "{}", file2);
    assert!(file2.contains("type: bug"), "{}", file2);
    assert!(file2.contains("# GPU out of memory"), "{}", file2);

    // Polling again creates nothing new.
    mac.poll_once(&[]);
    assert_eq!(mac.ticket_files().len(), 2);

    // --no-materialize leaves raises alone.
    let r3 = vert.ok(&["queue", "raise", "Later", "--repo", "alpha"]);
    mac.poll_once(&["--no-materialize"]);
    assert_eq!(mac.ticket_files().len(), 2);
    assert_eq!(mac.entry(&r3)["state"], "pending");
}

#[test]
fn test_queue_two_clients_share_one_queue() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let alpha = Client::new("alpha", &server.url, "mac");
    let beta = Client::new("beta", &server.url, "vert");
    let watcher = Client::new("watcher", &server.url, "mac");

    let a = alpha.ok(&["create", "From alpha"]);
    let b = beta.ok(&["create", "From beta"]);
    beta.ok(&["close", &b]);

    let events = watcher.poll_once(&[]);
    assert_eq!(seqs(&events), [1, 2, 3]);
    assert_eq!(events[0]["repo"], "alpha");
    assert_eq!(events[1]["repo"], "beta");
    assert_eq!(events[2]["host"], "vert");
    assert_eq!(alpha.entry(&b)["ticket"]["status"], "closed");
    assert_eq!(beta.entry(&a)["ticket"]["title"], "From alpha");

    let alpha_only = jsonl(&watcher.ok(&["queue", "ls", "--json", "--repo", "alpha"]));
    assert_eq!(alpha_only.len(), 1);
    assert!(watcher.ok(&["queue", "ls"]).contains("beta/"));
}

#[test]
fn test_queue_only_one_poller_creates_a_raise() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    // Two checkouts of the same repo on two hosts.
    let first = Client::new("alpha", &server.url, "mac").init_repo();
    let second = Client::new("alpha", &server.url, "laptop").init_repo();
    let raiser = Client::new("batch", &server.url, "vert");

    let r = raiser.ok(&["queue", "raise", "Once only", "--repo", "alpha"]);
    first.poll_once(&[]);
    second.poll_once(&[]);
    assert_eq!(first.ticket_files().len(), 1);
    assert!(second.ticket_files().is_empty());
    let e = first.entry(&r);
    assert_eq!(e["state"], "materialized");
    assert_eq!(e["claimed_by"], "mac");
}

#[test]
fn test_queue_unavailable_create_still_succeeds() {
    // A port that was free a moment ago: nothing listens on it.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mac = Client::new("alpha", &format!("http://127.0.0.1:{}", port), "mac");

    let out = mac.run(&["create", "Offline"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let id = text(&out.stdout);
    assert!(mac.dir.join(".tickets").join(format!("{}.md", id)).exists());
    let err = text(&out.stderr);
    assert!(err.contains("queue update failed"), "{}", err);
    assert!(err.contains(&id), "{}", err);

    let out = mac.run(&["start", &id]);
    assert!(out.status.success());
    assert!(text(&out.stderr).contains("queue update failed"));
    assert!(mac.ok(&["query"]).contains("in_progress"));

    for args in [
        vec!["queue", "poll", "--once"],
        vec!["queue", "ls"],
        vec!["queue", "raise", "x", "--repo", "alpha"],
    ] {
        let out = mac.run(&args);
        assert!(
            !out.status.success(),
            "{:?} should fail when the queue is down",
            args
        );
        assert!(
            text(&out.stderr).contains("cannot connect"),
            "{}",
            text(&out.stderr)
        );
    }
}

#[test]
fn test_queue_not_configured_keeps_offline_behaviour() {
    let mac = Client::new("alpha", "", "mac");
    let out = mac
        .command(&["create", "Plain"])
        .env_remove("TK_QUEUE_URL")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "",
        "no queue configured must print nothing extra"
    );

    let out = mac
        .command(&["queue", "poll", "--once"])
        .env_remove("TK_QUEUE_URL")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("TK_QUEUE_URL is not set"));
}

#[test]
fn test_queue_poll_cursor_once_since_exec() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let mac = Client::new("alpha", &server.url, "mac");
    mac.ok(&["create", "one"]);
    let two = mac.ok(&["create", "two"]);

    // The saved cursor moves forward.
    assert_eq!(seqs(&mac.poll_once(&[])), [1, 2]);
    assert!(mac.poll_once(&[]).is_empty());
    mac.ok(&["create", "three"]);
    assert_eq!(seqs(&mac.poll_once(&[])), [3]);
    assert_eq!(
        std::fs::read_to_string(mac.state.join("cursor-alpha"))
            .unwrap()
            .trim(),
        "3"
    );

    // --since replays from a given seq and saves the new cursor.
    assert_eq!(seqs(&mac.poll_once(&["--since", "1"])), [2, 3]);

    // --exec runs once per change, with the change on stdin and in env.
    let out_file = mac.dir.join("events.jsonl");
    let seen_file = mac.dir.join("seen.txt");
    let cmd = format!(
        "cat >> {}; echo \"$TK_QUEUE_SEQ $TK_QUEUE_OP $TK_QUEUE_KIND $TK_QUEUE_REPO $TK_QUEUE_ID\" >> {}",
        out_file.display(),
        seen_file.display()
    );
    let cursor = mac.dir.join("exec.cursor");
    let cursor_arg = cursor.to_string_lossy().to_string();
    mac.poll_once(&["--since", "0", "--cursor", &cursor_arg, "--exec", &cmd]);
    assert_eq!(
        seqs(&jsonl(&std::fs::read_to_string(&out_file).unwrap())),
        [1, 2, 3]
    );
    let seen = std::fs::read_to_string(&seen_file).unwrap();
    assert!(
        seen.contains(&format!("2 create ticket alpha {}", two)),
        "{}",
        seen
    );
    assert_eq!(std::fs::read_to_string(&cursor).unwrap().trim(), "3");

    // A failing --exec stops the poll, exits non-zero, and keeps the cursor before that change.
    let fail_cursor = mac.dir.join("fail.cursor");
    let fail_arg = fail_cursor.to_string_lossy().to_string();
    let out = mac.run(&[
        "queue",
        "poll",
        "--once",
        "--since",
        "0",
        "--cursor",
        &fail_arg,
        "--exec",
        "test \"$TK_QUEUE_SEQ\" -lt 2",
    ]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("--exec command failed"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&fail_cursor).unwrap().trim(), "1");
}

#[test]
fn test_queue_poll_loop_sees_new_changes() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let mac = Client::new("alpha", &server.url, "mac");

    let mut poller = mac
        .command(&["queue", "poll", "--interval", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let lines = spawn_line_reader(poller.stdout.take().unwrap());
    let id = mac.ok(&["create", "while polling"]);
    let got = lines.recv_timeout(WAIT);
    let _ = poller.kill();
    let _ = poller.wait();
    let line = got.expect("poll loop printed nothing within the wait");
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["id"],
        id.as_str()
    );
}

#[test]
fn test_queue_server_restart_keeps_entries_and_seq() {
    let data = data_dir();
    let log = data.path().join("q.jsonl");
    let id;
    {
        let server = Server::start(&log);
        let mac = Client::new("alpha", &server.url, "mac");
        id = mac.ok(&["create", "survives"]);
        mac.ok(&["close", &id]);
    }
    let server = Server::start(&log);
    let mac = Client::new("alpha", &server.url, "mac");
    assert_eq!(mac.entry(&id)["ticket"]["status"], "closed");
    let next = mac.ok(&["create", "after restart"]);
    assert_eq!(mac.entry(&next)["seq"], 3);

    // A second server on the same log is refused.
    let mut second = Command::new(bin())
        .args(["queue", "serve", "--port", "0", "--data"])
        .arg(&log)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = second.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() > deadline {
            let _ = second.kill();
            let _ = second.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let status = status.expect("second server on the same log kept running");
    assert!(!status.success());
    let mut err = String::new();
    second
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(err.contains("in use"), "{}", err);
}

/// Send raw request text and return the status code.
fn raw_status(url: &str, request: &str) -> u16 {
    let mut s = TcpStream::connect(url.strip_prefix("http://").unwrap()).unwrap();
    s.set_read_timeout(Some(WAIT)).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    raw.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn test_queue_server_refuses_browser_style_requests() {
    let data = data_dir();
    let server = Server::start(&data.path().join("q.jsonl"));
    let body = "repo=alpha&title=injected";
    let form = |host: &str, extra: &str| {
        format!(
            "POST /raise HTTP/1.1\r\nHost: {}\r\n{}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            host, extra, body.len(), body
        )
    };
    // A cross-site form post cannot add the client header.
    assert_eq!(raw_status(&server.url, &form("127.0.0.1:7878", "")), 403);
    // A DNS-rebinding page sends its own host name.
    assert_eq!(
        raw_status(&server.url, &form("evil.example:7878", "X-TK-Queue: 1\r\n")),
        403
    );
    assert_eq!(
        raw_status(
            &server.url,
            "GET /entries HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n"
        ),
        403
    );
    // curl through the tunnel: Host is 127.0.0.1 or localhost.
    assert_eq!(
        raw_status(&server.url, &form("localhost:7878", "X-TK-Queue: 1\r\n")),
        200
    );
    assert_eq!(
        raw_status(
            &server.url,
            "GET /entries HTTP/1.1\r\nHost: 127.0.0.1:7878\r\nConnection: close\r\n\r\n"
        ),
        200
    );
}
