//! Minimal HTTP/1.1 over `std::net` for the ticket queue.
//!
//! Only what `tk queue` needs: one request per connection (`Connection: close`),
//! `Content-Length` bodies, no chunked encoding, no TLS. Kept dependency-free
//! on purpose; the server only ever binds 127.0.0.1.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEADER_BYTES: u64 = 16 * 1024;
/// Largest response body a client accepts (events are paged, so this is generous).
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
/// Header every tk client sends. Browsers cannot add it to a cross-site
/// request without a CORS preflight (which the server never answers), so
/// the server requires it on writes.
pub const CLIENT_HEADER: &str = "X-TK-Queue";

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub content_type: String,
    /// `Host` header without the port, lower case.
    pub host: String,
    /// Whether the request carried `CLIENT_HEADER`.
    pub from_client: bool,
    pub body: Vec<u8>,
}

/// Read one request from `stream`; rejects bodies over `max_body` bytes.
pub fn read_request(stream: &TcpStream, max_body: usize) -> Result<Request> {
    let mut reader = BufReader::new(stream.take(MAX_HEADER_BYTES + max_body as u64));

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| anyhow!("empty request"))?
        .to_string();
    let target = parts
        .next()
        .ok_or_else(|| anyhow!("missing request target"))?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), form_decode(q)),
        None => (target.to_string(), HashMap::new()),
    };

    let mut content_length = 0usize;
    let mut content_type = String::new();
    let mut host = String::new();
    let mut from_client = false;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("connection closed inside headers");
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.parse().context("bad Content-Length")?,
                "content-type" => content_type = value.to_ascii_lowercase(),
                "host" => host = host_without_port(value).to_ascii_lowercase(),
                n if n.eq_ignore_ascii_case(CLIENT_HEADER) => from_client = true,
                _ => {}
            }
        }
    }
    if content_length > max_body {
        bail!(
            "body of {} bytes is over the {} byte limit",
            content_length,
            max_body
        );
    }
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .context("body shorter than Content-Length")?;

    Ok(Request {
        method,
        path,
        query,
        content_type,
        host,
        from_client,
        body,
    })
}

fn host_without_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("");
    }
    host.rsplit_once(':').map_or(host, |(h, _)| h)
}

pub fn write_response(
    mut stream: &TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason(status),
        content_type,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    }
}

/// Send one request to `base_url` (`http://host:port`) and return `(status, body)`.
pub fn call(
    base_url: &str,
    method: &str,
    path_and_query: &str,
    json_body: Option<&str>,
) -> Result<(u16, String)> {
    let authority = base_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("queue URL must start with http:// (got {})", base_url))?
        .trim_end_matches('/');
    if authority.contains('/') {
        bail!(
            "queue URL must be http://host:port with no path (got {})",
            base_url
        );
    }

    let stream = connect(authority)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let body = json_body.unwrap_or("");
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\n{}: 1\r\nConnection: close\r\nContent-Length: {}\r\n",
        method,
        path_and_query,
        authority,
        CLIENT_HEADER,
        body.len()
    );
    if json_body.is_some() {
        request.push_str("Content-Type: application/json\r\n");
    }
    request.push_str("\r\n");
    request.push_str(body);
    (&stream).write_all(request.as_bytes())?;

    let mut raw = Vec::new();
    stream.take(MAX_RESPONSE_BYTES).read_to_end(&mut raw)?;
    parse_response(&raw)
}

fn connect(authority: &str) -> Result<TcpStream> {
    let addrs = authority
        .to_socket_addrs()
        .with_context(|| format!("cannot resolve {}", authority))?;
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => Err(anyhow!("cannot connect to {}: {}", authority, e)),
        None => bail!("no address for {}", authority),
    }
}

fn parse_response(raw: &[u8]) -> Result<(u16, String)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("malformed HTTP response"))?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("malformed HTTP status line"))?;
    Ok((status, body.to_string()))
}

/// Decode `application/x-www-form-urlencoded` (also used for query strings).
pub fn form_decode(s: &str) -> HashMap<String, String> {
    s.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
            {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Percent-encode a query-string value (RFC 3986 unreserved characters pass through).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_decode() {
        let m = form_decode("title=Batch+failed&description=a%20b%26c%3D%0A&empty=&bad=%zz");
        assert_eq!(m["title"], "Batch failed");
        assert_eq!(m["description"], "a b&c=\n");
        assert_eq!(m["empty"], "");
        assert_eq!(m["bad"], "%zz");
    }

    #[test]
    fn test_percent_round_trip() {
        let s = "repo name/ü&x=1";
        assert_eq!(form_decode(&format!("k={}", percent_encode(s)))["k"], s);
    }

    #[test]
    fn test_percent_decode_trailing_percent() {
        assert_eq!(percent_decode("50%"), "50%");
        assert_eq!(percent_decode("5%2"), "5%2");
    }

    #[test]
    fn test_host_without_port() {
        assert_eq!(host_without_port("127.0.0.1:7878"), "127.0.0.1");
        assert_eq!(host_without_port("localhost"), "localhost");
        assert_eq!(host_without_port("[::1]:7878"), "::1");
    }

    #[test]
    fn test_parse_response() {
        let (status, body) =
            parse_response(b"HTTP/1.1 409 Conflict\r\nA: b\r\n\r\n{\"x\":1}").unwrap();
        assert_eq!(status, 409);
        assert_eq!(body, "{\"x\":1}");
    }
}
