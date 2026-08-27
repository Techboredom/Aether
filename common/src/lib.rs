use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Basic resource and status info for a single pod, aggregated across its containers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PodInfo {
    pub name: String,
    pub namespace: String,
    pub node: Option<String>,
    pub phase: String,
    pub ready_containers: u32,
    pub total_containers: u32,
    pub restarts: i32,
    /// RFC3339 pod start time; the frontend derives "age" from this at render time.
    pub start_time: Option<String>,
    pub container_images: Vec<String>,
    pub cpu_request_millicores: Option<i64>,
    pub cpu_limit_millicores: Option<i64>,
    pub memory_request_bytes: Option<i64>,
    pub memory_limit_bytes: Option<i64>,
    /// e.g. "nvidia.com/gpu" -> 1, "amd.com/gpu" -> 2, summed across containers.
    pub accelerators: BTreeMap<String, i64>,
}

/// Messages sent from backend to frontend over the pods WebSocket.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PodEvent {
    /// Full state, sent once when a client connects.
    Snapshot { pods: Vec<PodInfo> },
    /// A pod was added or changed.
    Upsert { pod: Box<PodInfo> },
    /// A pod was deleted, identified by name (unique within the watched namespace).
    Delete { name: String },
}
