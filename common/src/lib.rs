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

/// Submitted by the Images admin tab to create or update a catalog entry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveImageRequest {
    pub name: String,
    pub image: String,
    pub description: String,
}

/// Submitted by the "create deployment" form.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CreateDeploymentRequest {
    pub name: String,
    /// Name of the template this was launched from, if any (`None` for a
    /// Custom launch) — purely descriptive, carried through to `launch_log`
    /// for support/metrics; doesn't affect how the deployment is built.
    #[serde(default)]
    pub template_name: Option<String>,
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
    /// If set, the app is also reachable via Aether's own `/proxy/<name>/`
    /// route, which injects the generated credential automatically (if any).
    /// Requires `container_port` to be set. Comes from the selected
    /// template's `proxy_enabled`.
    #[serde(default)]
    pub enable_proxy: bool,
    /// Whether the proxy should forward the full `/proxy/<name>/...` path to
    /// the container as-is (`false`, e.g. JupyterLab's `base_url`) or strip
    /// that prefix first (`true`, e.g. RStudio's `www-root-path`, which only
    /// stamps the prefix onto outgoing redirects/cookies and still expects
    /// requests at the bare path). Only meaningful when `enable_proxy` is
    /// set. Comes from the selected template's `strip_prefix`.
    #[serde(default)]
    pub strip_prefix: bool,
    /// Whether Launch creates a public `LoadBalancer` Service (`true`,
    /// default) or a `ClusterIP`-only one (`false`) — must be `false` for
    /// any app with no auth of its own, since Aether's proxy ownership
    /// check then becomes the only thing gating access. Comes from the
    /// selected template's `public_service`.
    #[serde(default = "default_true")]
    pub public_service: bool,
}

fn default_true() -> bool {
    true
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
    /// Whether `service_name` (if any) is a public `LoadBalancer` or a
    /// `ClusterIP`-only Service — the frontend uses this to avoid telling
    /// someone to go check `kubectl get svc` for an external IP that
    /// doesn't exist.
    pub public_service: bool,
}

/// Current editable state of a running Deployment, returned by `GET
/// /api/deployments/{name}` to pre-fill the Pods tab's manage panel.
/// Image, container port, accelerator, and args are fixed at launch time —
/// changing those is a delete + relaunch, not an edit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeploymentDetail {
    pub name: String,
    pub replicas: i32,
    pub cpu_request: Option<String>,
    pub cpu_limit: Option<String>,
    pub memory_request: Option<String>,
    pub memory_limit: Option<String>,
    /// User-editable env vars — excludes the auto-generated secret's entry,
    /// if any (see `generated_secret_key`).
    pub env: Vec<(String, String)>,
    /// The env var key managed by an auto-generated secret (e.g.
    /// `"JUPYTER_TOKEN"`), if this deployment has one. Shown read-only in
    /// the manage panel rather than as an editable row, since its value is
    /// generated server-side and would otherwise be silently overwritten or
    /// blanked by a resubmit of `env`.
    pub generated_secret_key: Option<String>,
}

/// Submitted to `PUT /api/deployments/{name}` to scale and/or update
/// resources and env vars on an existing Deployment.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateDeploymentRequest {
    pub replicas: i32,
    pub cpu_request: Option<String>,
    pub cpu_limit: Option<String>,
    pub memory_request: Option<String>,
    pub memory_limit: Option<String>,
    pub env: Vec<(String, String)>,
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
    /// If true, this app is also reachable via Aether's `/proxy/<name>/`
    /// route, with `secret_env_key`'s generated value injected automatically
    /// (if any) — no separate login for that path.
    pub proxy_enabled: bool,
    /// Whether the proxy strips the `/proxy/<name>/` prefix before
    /// forwarding to the container. See `CreateDeploymentRequest::strip_prefix`.
    pub strip_prefix: bool,
    /// Whether Launch creates a public `LoadBalancer` Service (default) or
    /// a `ClusterIP`-only one. Must be `false` for templates with no
    /// `secret_env_key` and no other auth of their own (e.g. RStudio run
    /// with `DISABLE_AUTH=true`), since Aether's own login is then the only
    /// gate.
    pub public_service: bool,
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
    pub strip_prefix: bool,
    pub public_service: bool,
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

/// Submitted by the Users admin tab to reset another account's password.
/// Requires no proof of the old one — the admin role is the authorization.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResetPasswordRequest {
    pub password: String,
}

/// Submitted by the logged-in user themselves to change their own password.
/// Requires `current_password` to match, unlike an admin's reset.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// One row of `GET /api/sessions` — a past login. `username` is always
/// present; the frontend hides that column for non-admins the same way
/// `PodInfo::owner` is hidden on the Pods tab, since a `user`-role account's
/// query is already server-side filtered to just their own rows anyway.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionLogEntry {
    pub username: String,
    /// RFC3339-ish timestamp string (Postgres's default `timestamptz` text form).
    pub created_at: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// One row of `GET /api/launches` — a past Launch-tab submission, kept for
/// support/metrics ("who launched JupyterLab with what resources"). Same
/// admin-vs-own visibility split as `SessionLogEntry`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaunchLogEntry {
    pub username: String,
    pub created_at: String,
    pub deployment_name: String,
    pub template_name: Option<String>,
    pub image: String,
    pub replicas: i32,
    pub cpu_request: Option<String>,
    pub cpu_limit: Option<String>,
    pub memory_request: Option<String>,
    pub memory_limit: Option<String>,
    pub accelerator_type: Option<String>,
    pub accelerator_count: Option<i64>,
    pub container_port: Option<i32>,
    /// Same `(key, value)` shape as `TemplateEntry::env`. Any value matching
    /// the launch's `generate_secret_for` key was redacted before this was
    /// ever written to the database — see `deployments.rs`.
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
}

/// A CPU/memory/GPU limit triple, `None` meaning unlimited for that
/// dimension. Shared shape for both the global default and a per-user
/// override — see `common::MyQuota`/`UserQuotaEntry`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuotaLimits {
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
    pub gpu_limit: Option<i32>,
}

/// The cluster-wide default quota (`GET`/`PUT /api/quota/settings`,
/// admin-only to write). Applies to any user with no override row of their
/// own in `user_quotas`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuotaSettings {
    #[serde(flatten)]
    pub limits: QuotaLimits,
    /// Whether the Launch tab and the Pods tab's manage panel show
    /// separate CPU/memory *request* fields at all, independent of the
    /// quota limits themselves. When `false`, only limits are shown/sent,
    /// and a fixed request is substituted server-side (see below) instead
    /// of leaving it for Kubernetes to default to match the limit.
    pub expose_resource_requests: bool,
    /// Applied to every launch/edit's CPU/memory request in place of
    /// whatever the user would have set, but only while
    /// `expose_resource_requests` is `false` — irrelevant otherwise, since
    /// the user sets their own request directly in that mode. `None`
    /// means "leave the request unset", letting Kubernetes default it to
    /// match the limit.
    pub fixed_cpu_request: Option<String>,
    pub fixed_memory_request: Option<String>,
}

/// Returned by `GET /api/quota/me` — the caller's own effective quota,
/// current usage, and whether request fields should be shown at all.
/// Backs the Launch tab and the Pods tab's manage panel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MyQuota {
    /// The effective limits: the caller's `user_quotas` override if they
    /// have one, otherwise the global `quota_settings` default. Always
    /// unlimited (all `None`) for an admin, who is exempt from enforcement.
    pub limits: QuotaLimits,
    pub is_override: bool,
    pub expose_resource_requests: bool,
    /// Purely informational — the frontend never sends these back, the
    /// backend applies them server-side. See `QuotaSettings::fixed_cpu_request`.
    pub fixed_cpu_request: Option<String>,
    pub fixed_memory_request: Option<String>,
    pub used_cpu_millicores: i64,
    pub used_memory_bytes: i64,
    pub used_gpu_count: i64,
}

/// One row of the Quotas admin tab's per-user table (`GET /api/quota/users`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserQuotaEntry {
    pub user_id: i32,
    pub username: String,
    /// `None` if this user has no override and is bound by the global default.
    pub quota_override: Option<QuotaLimits>,
    pub used_cpu_millicores: i64,
    pub used_memory_bytes: i64,
    pub used_gpu_count: i64,
}
