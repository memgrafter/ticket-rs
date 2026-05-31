//! Ticket ID generation and resolution.
//!
//! Id format: `<prefix>-<suffix>`
//! - prefix: first letter of each hyphen/underscore segment of directory name (or first 3 chars)
//! - suffix: 4-char lower-case alphanumeric random string, e.g. "nw-5c46", "tk-ab12"

use anyhow::{bail, Result};

/// Generate a ticket ID from the current directory name + random suffix.
pub fn generate_id(dir_name: &str) -> String {
    let prefix = dir_name
        .split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().next().unwrap())
        .collect::<String>();

    let prefix = if prefix.len() < 2 {
        let chars: Vec<char> = dir_name.chars().collect();
        chars.iter().take(3).collect()
    } else {
        prefix
    };

    let suffix: String = (0..4)
        .map(|_| {
            let idx = fastrand::usize(..36);
            b"abcdefghijklmnopqrstuvwxyz0123456789"[idx] as char
        })
        .collect();

    format!("{}-{}", prefix, suffix)
}

/// Resolve a partial ID to a full ticket ID.
///
/// Matching order:
/// 1. Exact match
/// 2. Suffix match (id ends with the query)
/// 3. Prefix match (id starts with the query)
/// 4. Substring match (id contains the query)
///
/// Returns an error if ambiguous (>1 match) or not found.
pub fn resolve_id<'a>(ids: &'a [String], partial: &str) -> Result<&'a String> {
    let partial = partial.trim();

    // 1. Exact match
    if let Some(id) = ids.iter().find(|id| id.as_str() == partial) {
        return Ok(id);
    }

    // 2. Partial match: suffix, prefix, then substring
    let matches: Vec<&String> = ids
        .iter()
        .filter(|id| {
            id.ends_with(partial) || id.starts_with(partial) || id.contains(partial)
        })
        .collect();

    match matches.len() {
        0 => bail!("ticket '{}' not found", partial),
        1 => Ok(matches[0]),
        _ => {
            let ids: Vec<&str> = matches.iter().map(|s| s.as_str()).collect();
            bail!(
                "ambiguous ID '{}' matches multiple tickets: {}",
                partial,
                ids.join(", ")
            )
        }
    }
}

/// Generate the filename for a ticket ID.
pub fn ticket_filename(id: &str) -> String {
    format!("{}.md", id)
}

/// Extract the ticket ID from a filename.
pub fn ticket_id_from_filename(filename: &str) -> Option<String> {
    filename
        .strip_suffix(".md")
        .map(|s| s.to_string())
}

#[allow(dead_code)]
/// Validate that a string looks like a valid ticket ID.
pub fn is_valid_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    // Must match pattern: prefix-suffix where suffix is 4 alphanum
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return false;
    }
    let suffix = parts[1];
    if suffix.len() != 4 {
        return false;
    }
    suffix.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_id_produces_valid_format() {
        let id = generate_id("my-project");
        assert!(id.contains('-'));
        let parts: Vec<&str> = id.splitn(2, '-').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].len(), 4);
        assert!(parts[1].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_generate_id_multi_segment_prefix() {
        let id = generate_id("my-awesome-project");
        let prefix = id.split('-').next().unwrap();
        assert_eq!(prefix, "map");
    }

    #[test]
    fn test_generate_id_underscore_prefix() {
        let id = generate_id("my_awesome_project");
        let prefix = id.split('-').next().unwrap();
        assert_eq!(prefix, "map");
    }

    #[test]
    fn test_generate_id_single_segment_fallback() {
        let id = generate_id("ab");
        let prefix = id.split('-').next().unwrap();
        assert_eq!(prefix, "ab");
    }

    #[test]
    fn test_resolve_id_exact() {
        let ids = vec!["abc-1234".into(), "def-5678".into()];
        let result = resolve_id(&ids, "abc-1234").unwrap();
        assert_eq!(result, "abc-1234");
    }

    #[test]
    fn test_resolve_id_suffix() {
        let ids = vec!["abc-1234".into(), "def-5678".into()];
        let result = resolve_id(&ids, "1234").unwrap();
        assert_eq!(result, "abc-1234");
    }

    #[test]
    fn test_resolve_id_prefix() {
        let ids = vec!["abc-1234".into(), "def-5678".into()];
        let result = resolve_id(&ids, "abc").unwrap();
        assert_eq!(result, "abc-1234");
    }

    #[test]
    fn test_resolve_id_substring() {
        let ids = vec!["abc-1234".into(), "def-5678".into()];
        let result = resolve_id(&ids, "c-1").unwrap();
        assert_eq!(result, "abc-1234");
    }

    #[test]
    fn test_resolve_id_ambiguous() {
        let ids = vec!["abc-1234".into(), "abc-5678".into()];
        let result = resolve_id(&ids, "abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ambiguous"));
    }

    #[test]
    fn test_resolve_id_not_found() {
        let ids = vec!["abc-1234".into()];
        let result = resolve_id(&ids, "nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_resolve_id_exact_takes_precedence() {
        let ids = vec!["abc".into(), "abc-1234".into()];
        let result = resolve_id(&ids, "abc").unwrap();
        assert_eq!(result, "abc");
    }

    #[test]
    fn test_ticket_filename_roundtrip() {
        let id = "nw-5c46";
        let filename = ticket_filename(id);
        assert_eq!(filename, "nw-5c46.md");
        assert_eq!(ticket_id_from_filename(&filename).unwrap(), id);
    }

    #[test]
    fn test_is_valid_id() {
        assert!(is_valid_id("nw-5c46"));
        assert!(is_valid_id("tk-ab12"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("short"));
        assert!(!is_valid_id("too-long-suffix-12345"));
    }
}