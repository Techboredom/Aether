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
    pub containers: Vec<ContainerStatusInfo>,
    pub cpu_request_millicores: Option<i64>,
    pub cpu_limit_millicores: Option<i64>,
    pub memory_request_bytes: Option<i64>,
    pub memory_limit_bytes: Option<i64>,
    /// e.g. "nvidia.com/gpu" -> 1, "amd.com/gpu" -> 2, summed across containers.
    pub accelerators: BTreeMap<String, i64>,
    /// Username of whoever launched this pod via the Launch tab, if any
    /// (pods that predate this feature, or weren't launched through Aether,
    /// have no owner label and so show `None`).
    pub owner: Option<String>,
    /// The stable Deployment name (the `app` label), unlike the pod's own
    /// name which gets a random suffix and changes across restarts.
    pub deployment_name: Option<String>,
    /// The auto-generated login credential for this instance (JupyterLab
    /// token, RStudio password, vLLM API key, ...), if its template has one.
    /// Only populated for pods the requester is allowed to see.
    pub credential: Option<PodCredential>,
    /// If its template is proxy-enabled, the root-relative path
    /// (`/proxy/<deployment-name>/`) that opens it through Aether itself
    /// with the credential already injected — no login prompt, no public IP.
    pub proxy_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PodCredential {
    pub env_key: String,
    pub value: String,
}

/// Per-container status, surfaced so the UI can explain *why* a pod isn't healthy
/// (waiting/terminated reason + message) without a separate round trip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerStatusInfo {
    pub name: String,
    pub image: String,
    pub ready: bool,
    pub restart_count: i32,
    /// "running" | "waiting" | "terminated" | "unknown"
    pub state: String,
    /// e.g. "CrashLoopBackOff", "ImagePullBackOff", "OOMKilled".
    pub reason: Option<String>,
    pub message: Option<String>,
    pub exit_code: Option<i32>,
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

/// A container image available to pick from in the "create deployment" form,
/// backed by a row in the `images` Postgres table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageEntry {
    pub id: i32,
    pub name: String,
    pub image: String,
    pub description: String,
}

/// Submitted by the "create deployment" form.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CreateDeploymentRequest {
    pub name: String,
    pub image: String,
    pub replicas: i32,
    pub cpu_request: Option<String>,
    pub cpu_limit: Option<String>,
    pub memory_request: Option<String>,
    pub memory_limit: Option<String>,
    /// Accelerator resource name, e.g. "nvidia.com/gpu" or "amd.com/gpu".
    pub accelerator_type: Option<String>,
    pub accelerator_count: Option<i64>,
    /// If set, a `Service` (type LoadBalancer) is also created exposing this
    /// container port, since there's no ingress controller in the cluster yet.
    #[serde(default)]
    pub container_port: Option<i32>,
    /// Extra environment variables. Entries with an empty value are dropped,
    /// so an app's own default behavior (e.g. an auto-generated password
    /// logged at startup) still applies unless a value is explicitly set.
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Extra container command-line arguments (e.g. `--model=...` for vLLM/SGLang).
    #[serde(default)]
    pub args: Vec<String>,
    /// If set, the backend generates a random value and sets it as this env
    /// var (overriding any same-keyed entry in `env`), instead of the user
    /// typing one in — e.g. `"JUPYTER_TOKEN"`. Comes from the selected
    /// template's `secret_env_key`.
    #[serde(default)]
    pub generate_secret_for: Option<String>,
    /// If set, no public Service is created — the app is only reachable via
    /// Aether's own `/proxy/<name>/` route, which injects the generated
    /// credential automatically. Requires `generate_secret_for` and
    /// `container_port` to both be set. Comes from the selected template's
    /// `proxy_enabled`.
    #[serde(default)]
    pub enable_proxy: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateDeploymentResponse {
    pub name: String,
    pub namespace: String,
    /// Present if `container_port` was set and a matching Service was created.
    pub service_name: Option<String>,
    pub container_port: Option<i32>,
    /// The generated value, if `generate_secret_for` was set.
    pub secret_value: Option<String>,
    /// Present if `enable_proxy` was set — the root-relative path that opens
    /// this deployment through Aether with the credential already injected.
    pub proxy_path: Option<String>,
}

/// A Kubernetes Event involving a specific pod (scheduling failures, image pull
/// errors, OOM kills, etc. all surface here, often before it's visible any other way).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PodEventInfo {
    /// "Normal" or "Warning".
    pub type_: String,
    pub reason: String,
    pub message: String,
    pub count: i32,
    /// RFC3339 timestamp of the most recent occurrence, if known.
    pub last_seen: Option<String>,
}

/// A workload template (Ollama, JupyterLab, etc.), stored in the `templates`
/// table and managed from the Templates admin tab. Selecting one on the
/// Launch tab pre-fills a `CreateDeploymentRequest` with these values; every
/// field stays editable afterward.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TemplateEntry {
    pub id: i32,
    pub name: String,
    pub image: String,
    pub container_port: Option<i32>,
    pub cpu_request: String,
    pub cpu_limit: String,
    pub memory_request: String,
    pub memory_limit: String,
    pub accelerator_type: String,
    pub accelerator_count: Option<i64>,
    /// `(key, value)` pairs. A blank value is scaffolding for the launcher to
    /// fill in — see `CreateDeploymentRequest::env`.
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
    pub notes: String,
    /// If set, launching this template generates a random value for this env
    /// var automatically instead of showing it as an editable field — e.g.
    /// JupyterLab's `"JUPYTER_TOKEN"`, RStudio's `"PASSWORD"`.
    pub secret_env_key: Option<String>,
    /// If true, launching this template skips the public LoadBalancer
    /// Service and is only reachable via Aether's `/proxy/<name>/` route
    /// instead, with `secret_env_key`'s generated value injected
    /// automatically — no separate login. Requires `secret_env_key` to be set.
    pub proxy_enabled: bool,
}

/// Submitted by the Templates admin tab to create or update a template.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveTemplateRequest {
    pub name: String,
    pub image: String,
    pub container_port: Option<i32>,
    pub cpu_request: String,
    pub cpu_limit: String,
    pub memory_request: String,
    pub memory_limit: String,
    pub accelerator_type: String,
    pub accelerator_count: Option<i64>,
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
    pub notes: String,
    pub secret_env_key: Option<String>,
    pub proxy_enabled: bool,
}

/// The two account classes. Admins can manage templates and accounts; both
/// classes can view pods and launch deployments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

/// The logged-in user, as returned by `GET /api/me`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i32,
    pub username: String,
    pub role: Role,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Submitted by the Users admin tab to create an account.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Role,
}
