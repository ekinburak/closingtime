use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "closingtime.session.v1";
pub const SESSION_ENV: &str = "CLOSINGTIME_SESSION";
pub const PROJECT_ENV: &str = "CLOSINGTIME_PROJECT";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub host: String,
    pub boot: String,
    pub uid: u32,
    pub pid: u32,
    /// Kernel ticks on Linux; seconds:microseconds from proc_bsdinfo on macOS.
    pub start: String,
}
impl ProcessIdentity {
    pub fn key(&self) -> String {
        // JSON is an unambiguous key even when a host name contains punctuation.
        serde_json::to_string(self).expect("identity serialization")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub identity: ProcessIdentity,
    pub parent_pid: u32,
    pub executable: Option<String>,
    pub name: String,
    pub zombie: bool,
    pub session_tag: Option<String>,
    pub tag_readable: bool,
    /// None means manager detection failed, not 'unmanaged'.
    pub manager_owned: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Port {
    pub pid: u32,
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub processes: BTreeMap<u32, Process>,
    pub unavailable: BTreeMap<u32, String>,
    pub ports: Vec<Port>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub label: String,
    /// Program name only, never arguments. Supplied when the library launches the root.
    pub command: Option<String>,
    pub project: String,
    pub native_session_id: Option<String>,
    pub host: String,
    pub boot: String,
    pub uid: u32,
    pub supervisor: ProcessIdentity,
    pub root: Option<ProcessIdentity>,
    pub state: SessionState,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    LaunchRegistration,
    ExplicitRegistration,
    InheritedTag,
    ObservedAncestry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipRecord {
    pub identity: ProcessIdentity,
    pub session_id: String,
    pub parent_key: Option<String>,
    pub executable: Option<String>,
    pub name: String,
    pub evidence: Vec<Evidence>,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub conflict: bool,
    /// A positive service-manager observation stays report-only for this identity.
    #[serde(default)]
    pub manager_owned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceView {
    pub identity: ProcessIdentity,
    pub session_id: Option<String>,
    pub session_label: Option<String>,
    pub project: Option<String>,
    pub command: Option<String>,
    pub name: String,
    pub executable: Option<String>,
    pub evidence: Vec<Evidence>,
    pub ports: Vec<Port>,
    pub status: String,
    pub kept: bool,
    pub cleanup_eligible: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scan {
    pub schema: String,
    pub resources: Vec<ResourceView>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPlan {
    pub schema: String,
    pub session_id: String,
    pub created_ms: u64,
    pub resources: Vec<ResourceView>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub session_id: String,
    pub identity: ProcessIdentity,
    pub actor_uid: u32,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub result: String,
    pub signals: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Export {
    pub schema: String,
    pub sessions: Vec<Session>,
    pub processes: Vec<OwnershipRecord>,
    pub kept: BTreeSet<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone)]
pub enum Inspection {
    Present(Process),
    Gone,
    Unavailable(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doctor {
    pub platform: String,
    pub host: String,
    pub boot: String,
    pub uid: u32,
    pub identity_available: bool,
    pub tags_readable: bool,
    pub ports_available: bool,
    pub pidfd_available: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pruned {
    pub sessions: usize,
    pub processes: usize,
    pub keeps: usize,
    pub events: usize,
}
