//! Auto-generated from ticket.ts — DO NOT EDIT BY HAND.
//! Run `./generate-types.sh` to regenerate.
//!
//! All types correspond to interfaces exported from ticket.ts.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Priority: 0-4, 0=highest, default 2
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum Priority {
    P0 = 0,
    P1 = 1,
    P2 = 2,
    P3 = 3,
    P4 = 4,
}

impl Priority {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Priority::P0),
            1 => Some(Priority::P1),
            2 => Some(Priority::P2),
            3 => Some(Priority::P3),
            4 => Some(Priority::P4),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Status lifecycle: open → in_progress → closed (reopen goes back to open)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    #[serde(rename = "in_progress")]
    InProgress,
    Closed,
}

/// Type: bug, feature, task, epic, chore
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TicketType {
    Bug,
    Feature,
    Task,
    Epic,
    Chore,
}

impl FromStr for TicketType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "bug" => Ok(TicketType::Bug),
            "feature" => Ok(TicketType::Feature),
            "task" => Ok(TicketType::Task),
            "epic" => Ok(TicketType::Epic),
            "chore" => Ok(TicketType::Chore),
            _ => Err(format!("unknown type: {}", s)),
        }
    }
}

/// Frontmatter metadata for a ticket file.
/// Corresponds to ticket.ts `Metadata` / `Frontmatter`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub id: String,
    pub status: Status,
    pub deps: Vec<String>,
    pub links: Vec<String>,
    pub created: String,
    #[serde(rename = "type")]
    pub metadata_type: TicketType,
    pub priority: Priority,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(rename = "externalRef", skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// A timestamped note appended to a ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub timestamp: String,
    pub body: String,
}

/// Body data for a ticket file (markdown content after frontmatter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<Note>>,
}

/// Options for creating a new ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOptions {
    pub title: Option<String>,
    pub description: Option<String>,
    pub design: Option<String>,
    pub acceptance: Option<String>,
    #[serde(rename = "type")]
    pub create_type: Option<TicketType>,
    pub priority: Option<Priority>,
    pub assignee: Option<String>,
    pub external_ref: Option<String>,
    pub parent: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Filter for listing/searching tickets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    pub status: Option<Status>,
    pub assignee: Option<String>,
    #[serde(rename = "type")]
    pub filter_type: Option<TicketType>,
    pub tags: Option<Vec<String>>,
}
