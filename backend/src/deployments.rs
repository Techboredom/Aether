use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Json;
use common::{CreateDeploymentRequest, CreateDeploymentResponse, DeploymentDetail, LaunchLogEntry, PvcEntry, Role, UpdateDeploymentRequest};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, PersistentVolumeClaim, PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec,
    ResourceRequirements, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use sqlx::types::Json as SqlxJson;
use sqlx::FromRow;

use crate::auth::{generate_name_suffix, generate_token, CurrentUser};
use crate::error::ApiError;
use crate::quota;
use crate::resources::{parse_count, parse_cpu_millicores, parse_memory_bytes, OWNER_LABEL};
use crate::state::AppState;
use crate::validate;

/// Trims a client-supplied quantity and treats blank as absent.
///
/// The forms submit "" for a field the user cleared. Passing that straight
/// through would put an empty `Quantity` into the pod spec, which the API
/// server rejects with a message about the *resource list* rather than about
/// the field the user actually left empty — so normalize it to "unset" here,
/// which is what they meant.
pub fn normalize_quantity(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Lowercase, alphanumeric-and-hyphen only, runs of anything else collapsed
/// to a single '-' with none leading/trailing — turns a display string
/// (a template's name, or an image's repository component) into something
/// usable as part of a Kubernetes name.
fn slugify(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// A slug for the "instance type" segment of an auto-generated name when
/// there's no template to name it after (a Custom launch): the image's own
/// repository name, stripped of registry/org path, tag, and digest — e.g.
/// "ctr.example.com:8443/aether/aether:v1" and "jupyter/base-notebook" slug
/// to "aether" and "base-notebook" respectively.
fn image_repo_slug(image: &str) -> String {
    let repo = image.rsplit('/').next().unwrap_or(image);
    let repo = repo.split(['@', ':']).next().unwrap_or(repo);
    let slug = slugify(repo);
    if slug.is_empty() { "app".to_string() } else { slug }
}

/// Builds `<username>-<instance_type>-<random>` (the actual Deployment/
/// Service name), truncating `instance_type` as needed to stay within
/// Kubernetes' 63-character name limit — silently, since it's just a
/// descriptive slug and there's no user-facing field to ask them to
/// shorten. The random suffix is fixed-length and generated first so the
/// truncation budget for `instance_type` is exact.
fn scoped_deployment_name(username: &str, instance_type: &str) -> String {
    let suffix = generate_name_suffix();
    // Two separating hyphens, plus the username and suffix, are fixed
    // overhead; whatever's left is instance_type's budget.
    let budget = 63usize.saturating_sub(username.len() + suffix.len() + 2);
    let truncated: String = instance_type.chars().take(budget).collect();
    let truncated = truncated.trim_end_matches('-');
    format!("{username}-{truncated}-{suffix}")
}

/// Parses a user's admin-set "key=value" node label (validated at
/// write-time by `validate::node_label`, so this only ever sees a
/// well-formed value) into the single-entry map `PodSpec::node_selector`
/// expects. `None` if the user has no node label set.
fn node_selector_for(user: &CurrentUser) -> Option<BTreeMap<String, String>> {
    let (key, value) = user.node_label.as_deref()?.split_once('=')?;
    Some(BTreeMap::from([(key.to_string(), value.to_string())]))
}

/// Errors if `user` isn't allowed to manage `deployment` — an admin always
/// is; anyone else only for a deployment carrying their own `OWNER_LABEL`.
/// A deployment with no owner label at all (predates Aether, or wasn't
/// launched through it) can only be managed by an admin.
fn check_owner(deployment: &Deployment, user: &CurrentUser) -> Result<(), ApiError> {
    if user.role == Role::Admin {
        return Ok(());
    }
    let owner = deployment.metadata.labels.as_ref().and_then(|l| l.get(OWNER_LABEL));
    if owner.map(|o| o.as_str()) == Some(user.username.as_str()) {
        Ok(())
    } else {
        Err(ApiError::Forbidden("you don't own this deployment".to_string()))
    }
}

pub async fn create_deployment(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(req): Json<CreateDeploymentRequest>,
) -> Result<Json<CreateDeploymentResponse>, ApiError> {
    let mut req = req;
    req.cpu_request = normalize_quantity(req.cpu_request.take());
    req.cpu_limit = normalize_quantity(req.cpu_limit.take());
    req.memory_request = normalize_quantity(req.memory_request.take());
    req.memory_limit = normalize_quantity(req.memory_limit.take());

    validate::image_ref(&req.image)?;
    if req.replicas < 0 {
        return Err(ApiError::BadRequest("replicas must not be negative".into()));
    }
    if let Some(port) = req.container_port {
        validate::container_port(port)?;
    }
    for field in [
        ("cpu_request", &req.cpu_request),
        ("cpu_limit", &req.cpu_limit),
        ("memory_request", &req.memory_request),
        ("memory_limit", &req.memory_limit),
    ] {
        if let (name, Some(value)) = field {
            validate::quantity(name, value)?;
        }
    }
    for (key, _) in &req.env {
        if !key.trim().is_empty() {
            validate::env_key(key)?;
        }
    }
    validate::bounded_list("env", &req.env.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(), 50, 4096)?;
    validate::bounded_list("args", &req.args, 50, 1024)?;
    if let Some(key) = &req.generate_secret_for {
        validate::env_key(key)?;
    }
    if req.enable_proxy && req.container_port.is_none() {
        return Err(ApiError::BadRequest("enable_proxy requires container_port".into()));
    }
    validate::optional_text("model", req.model.as_deref().unwrap_or(""), 500)?;
    validate::volume_mount(req.volume_claim_name.as_deref().unwrap_or(""), req.volume_mount_path.as_deref().unwrap_or(""))?;
    if let Some(claim_name) = &req.volume_claim_name {
        // Shape was already checked above; this confirms it's real, so a
        // typo'd claim name fails fast with a clear 400 instead of leaving
        // the pod stuck Pending with nothing but a mount-failure event to
        // explain why.
        let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(state.client.clone(), &state.namespace);
        pvcs.get(claim_name).await.map_err(|err| match &err {
            kube::Error::Api(status) if status.code == 404 => {
                ApiError::BadRequest(format!("no PersistentVolumeClaim named \"{claim_name}\" in this namespace"))
            }
            _ => ApiError::from(err),
        })?;
    }

    let global_settings = quota::load_global_settings(&state.pg).await?;
    quota::check_image_allowed(&state, &user, &req.image, &global_settings).await?;

    let replicas = i64::from(req.replicas);
    let additional_cpu = req.cpu_limit.as_deref().and_then(|v| parse_cpu_millicores(&Quantity(v.to_string()))).unwrap_or(0) * replicas;
    let additional_memory = req.memory_limit.as_deref().and_then(|v| parse_memory_bytes(&Quantity(v.to_string()))).unwrap_or(0) * replicas;
    let additional_gpu = match (&req.accelerator_type, req.accelerator_count) {
        (Some(accel_type), Some(count)) if !accel_type.trim().is_empty() && count > 0 => count * replicas,
        _ => 0,
    };
    // No user-chosen name: <username>-<instance type>-<random>, so launching
    // never requires picking something unique yourself. "Instance type" is
    // the template's own name when launched from one, or a slug of the
    // image for a Custom launch (which has no template name to use
    // instead) — see scoped_deployment_name's own doc comment for how the
    // pieces are trimmed to fit Kubernetes' 63-character name limit.
    let instance_type = req
        .template_name
        .as_deref()
        .map(slugify)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| image_repo_slug(&req.image));
    let scoped_name = scoped_deployment_name(&user.username, &instance_type);

    // Held until the Deployment is actually created, so a concurrent launch
    // (on this replica or another) can't slip past the same quota check —
    // see AppState::lock_launches.
    let launch_tx = state.lock_launches().await?;
    quota::check_quota(&state, &user, None, additional_cpu, additional_memory, additional_gpu).await?;

    // When requests aren't exposed to the user, an admin-configured fixed
    // value stands in for whatever they would have set (which is nothing,
    // since the frontend doesn't even show the field in that mode) rather
    // than leaving it for Kubernetes to default the request to the limit.
    // (global_settings was already loaded above, for check_image_allowed.)
    let (cpu_request, memory_request) = if global_settings.expose_resource_requests {
        (req.cpu_request.clone(), req.memory_request.clone())
    } else {
        (global_settings.fixed_cpu_request.clone(), global_settings.fixed_memory_request.clone())
    };

    let mut requests = BTreeMap::new();
    let mut limits = BTreeMap::new();
    if let Some(v) = &cpu_request {
        requests.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.cpu_limit {
        limits.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &memory_request {
        requests.insert("memory".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.memory_limit {
        limits.insert("memory".to_string(), Quantity(v.clone()));
    }
    if let (Some(accel_type), Some(count)) = (&req.accelerator_type, req.accelerator_count)
        && !accel_type.trim().is_empty() && count > 0 {
            // Extended resources like GPUs require request == limit.
            let qty = Quantity(count.to_string());
            requests.insert(accel_type.clone(), qty.clone());
            limits.insert(accel_type.clone(), qty);
        }

    // A generated secret always wins over a same-keyed manual entry, so the
    // "auto-generate" behavior can't be silently bypassed via the generic env list.
    let mut env: Vec<EnvVar> = req
        .env
        .iter()
        .filter(|(key, value)| !value.trim().is_empty() && Some(key) != req.generate_secret_for.as_ref())
        .map(|(name, value)| EnvVar {
            name: name.clone(),
            value: Some(value.clone()),
            ..Default::default()
        })
        .collect();
    let generated_secret = req.generate_secret_for.as_ref().map(|key| {
        let value = generate_token();
        env.push(EnvVar { name: key.clone(), value: Some(value.clone()), ..Default::default() });
        value
    });
    // "{{name}}" refers to the deployment's own generated name, e.g.
    // JupyterLab's `--ServerApp.base_url=/proxy/{{name}}/`. "{{model}}" is
    // req.model (a Hugging Face ID or a path under volume_mount_path below —
    // just a string either way). "{{accelerator_count}}" is however many
    // GPUs were actually requested (defaulting to 1 if none were, so a
    // template whose args always reference it - e.g. vLLM's
    // --tensor-parallel-size - doesn't end up with a nonsensical 0).
    let model = req.model.clone().unwrap_or_default();
    let accelerator_count_str = req.accelerator_count.filter(|&c| c > 0).unwrap_or(1).to_string();
    let args: Vec<String> = req
        .args
        .iter()
        .map(|a| {
            a.trim()
                .replace("{{name}}", &scoped_name)
                .replace("{{model}}", &model)
                .replace("{{accelerator_count}}", &accelerator_count_str)
        })
        .filter(|a| !a.is_empty())
        .collect();
    // `args` gets moved into the Container below; keep a copy for launch_log.
    let logged_args = args.clone();

    // Mounts an existing PersistentVolumeClaim (already confirmed to exist,
    // above) into the container — e.g. a shared model cache, so `model`
    // above can be a local path instead of always re-downloading from
    // Hugging Face. `None` (no volume_claim_name) means neither field is
    // set at all, not empty Vecs, since an empty `volumes: []` vs. an
    // absent field can render differently depending on the API server
    // version and this is simpler to reason about either way.
    let (volumes, volume_mounts) = match &req.volume_claim_name {
        Some(claim_name) => (
            Some(vec![Volume {
                name: "data".to_string(),
                persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                    claim_name: claim_name.clone(),
                    read_only: Some(false),
                }),
                ..Default::default()
            }]),
            Some(vec![VolumeMount {
                name: "data".to_string(),
                mount_path: req.volume_mount_path.clone().unwrap_or_default(),
                sub_path: req.volume_sub_path.clone().filter(|s| !s.is_empty()),
                ..Default::default()
            }]),
        ),
        None => (None, None),
    };

    // The selector must stay a fixed, minimal set of labels the pod template
    // always carries; `owner` rides along as an extra descriptive label on
    // top of it (on the Deployment, its pods, and the Service), not part of
    // the selector itself.
    let mut selector_labels = BTreeMap::new();
    selector_labels.insert("app".to_string(), scoped_name.clone());
    let mut object_labels = selector_labels.clone();
    object_labels.insert(OWNER_LABEL.to_string(), user.username.clone());

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(scoped_name.clone()),
            namespace: Some(state.namespace.clone()),
            labels: Some(object_labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(req.replicas),
            selector: LabelSelector {
                match_labels: Some(selector_labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(object_labels.clone()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    node_selector: node_selector_for(&user),
                    volumes,
                    containers: vec![Container {
                        name: scoped_name.clone(),
                        image: Some(req.image.clone()),
                        resources: Some(ResourceRequirements {
                            requests: (!requests.is_empty()).then_some(requests),
                            limits: (!limits.is_empty()).then_some(limits),
                            ..Default::default()
                        }),
                        env: (!env.is_empty()).then_some(env),
                        args: (!args.is_empty()).then_some(args),
                        volume_mounts,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        status: None,
    };

    let deployments: Api<Deployment> = Api::namespaced(state.client.clone(), &state.namespace);
    let created = deployments.create(&PostParams::default(), &deployment).await.map_err(|err| {
        // The name includes a random suffix, so a 409 here would mean that
        // exact suffix collided for this exact user and instance type — a
        // one-in-billions coincidence worth surfacing plainly rather than
        // as a raw API-server error if it somehow happens.
        match &err {
            kube::Error::Api(status) if status.code == 409 => {
                ApiError::BadRequest(format!("\"{scoped_name}\" already exists — this should be extremely rare; try again"))
            }
            _ => ApiError::from(err),
        }
    })?;
    // The new footprint is now visible to the next quota check; everything
    // below is bookkeeping that doesn't affect it.
    launch_tx.commit().await?;
    let name = created.metadata.name.unwrap_or(scoped_name);

    // No ingress controller in the cluster yet, so expose the app directly via
    // its own Service — `LoadBalancer` (MetalLB assigns it an external IP) by
    // default, or `ClusterIP`-only when `public_service` is false. That's
    // required for apps with no auth of their own that rely on Aether's own
    // login as the gate (JupyterLab, RStudio), but it's also a valid, useful
    // choice on its own for templates with no proxy at all (Ollama/vLLM/
    // SGLang set to "internal") — cluster-internal callers (e.g. a coding
    // tool running as another pod) can still reach a ClusterIP directly, it
    // just isn't exposed outside the cluster. Created regardless of
    // `enable_proxy`, since Aether's own /proxy/ route (backend/src/proxy.rs)
    // reaches proxy-enabled deployments through this same Service's
    // in-cluster ClusterIP either way.
    let mut service_name = None;
    if let Some(port) = req.container_port {
        let service_type = if req.public_service { "LoadBalancer" } else { "ClusterIP" };
        let service = Service {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(state.namespace.clone()),
                labels: Some(object_labels),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some(service_type.to_string()),
                selector: Some(selector_labels),
                ports: Some(vec![ServicePort {
                    port,
                    target_port: Some(IntOrString::Int(port)),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            status: None,
        };
        let services: Api<Service> = Api::namespaced(state.client.clone(), &state.namespace);
        let created_service = services.create(&PostParams::default(), &service).await?;
        service_name = created_service.metadata.name;
    }

    // Tracks proxy-routing metadata (not just credentials) for any
    // proxy-enabled deployment, even ones with no generated secret at all
    // (e.g. RStudio run with DISABLE_AUTH=true, relying solely on the
    // ownership check below).
    if req.generate_secret_for.is_some() || req.enable_proxy {
        sqlx::query(
            "INSERT INTO deployment_secrets \
                (deployment_name, namespace, env_key, secret_value, owner_username, proxy_enabled, \
                 container_port, strip_prefix) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (deployment_name) DO UPDATE SET \
                namespace = EXCLUDED.namespace, env_key = EXCLUDED.env_key, \
                secret_value = EXCLUDED.secret_value, owner_username = EXCLUDED.owner_username, \
                proxy_enabled = EXCLUDED.proxy_enabled, container_port = EXCLUDED.container_port, \
                strip_prefix = EXCLUDED.strip_prefix",
        )
        .bind(&name)
        .bind(&state.namespace)
        .bind(&req.generate_secret_for)
        .bind(&generated_secret)
        .bind(&user.username)
        .bind(req.enable_proxy)
        .bind(req.container_port)
        .bind(req.strip_prefix)
        .execute(&state.pg)
        .await?;
    }

    // Kept for support/metrics ("who launched JupyterLab with what
    // resources, who ran vLLM with what model") — an append-only record,
    // unlike deployment_secrets which gets overwritten on re-launch. Any
    // generated-secret value is redacted before this is ever written, since
    // (unlike deployment_secrets) any logged-in user can see their own rows.
    let logged_env: Vec<(String, String)> = req
        .env
        .iter()
        .map(|(k, v)| {
            if Some(k) == req.generate_secret_for.as_ref() { (k.clone(), "<generated>".to_string()) } else { (k.clone(), v.clone()) }
        })
        .collect();
    sqlx::query(
        "INSERT INTO launch_log \
            (deployment_name, namespace, owner_username, template_name, image, replicas, \
             cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count, \
             container_port, env, args) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(&name)
    .bind(&state.namespace)
    .bind(&user.username)
    .bind(&req.template_name)
    .bind(&req.image)
    .bind(req.replicas)
    .bind(&req.cpu_request)
    .bind(&req.cpu_limit)
    .bind(&req.memory_request)
    .bind(&req.memory_limit)
    .bind(&req.accelerator_type)
    .bind(req.accelerator_count)
    .bind(req.container_port)
    .bind(SqlxJson(&logged_env))
    .bind(&logged_args)
    .execute(&state.pg)
    .await?;

    let proxy_path = req.enable_proxy.then(|| state.proxy_url(&name));

    Ok(Json(CreateDeploymentResponse {
        name,
        namespace: state.namespace,
        service_name,
        container_port: req.container_port,
        secret_value: generated_secret,
        proxy_path,
        public_service: req.public_service,
    }))
}

#[derive(FromRow)]
struct LaunchLogRow {
    username: String,
    created_at: String,
    deployment_name: String,
    template_name: Option<String>,
    image: String,
    replicas: i32,
    cpu_request: Option<String>,
    cpu_limit: Option<String>,
    memory_request: Option<String>,
    memory_limit: Option<String>,
    accelerator_type: Option<String>,
    accelerator_count: Option<i64>,
    container_port: Option<i32>,
    env: SqlxJson<Vec<(String, String)>>,
    args: Vec<String>,
}

impl From<LaunchLogRow> for LaunchLogEntry {
    fn from(row: LaunchLogRow) -> Self {
        LaunchLogEntry {
            username: row.username,
            created_at: row.created_at,
            deployment_name: row.deployment_name,
            template_name: row.template_name,
            image: row.image,
            replicas: row.replicas,
            cpu_request: row.cpu_request,
            cpu_limit: row.cpu_limit,
            memory_request: row.memory_request,
            memory_limit: row.memory_limit,
            accelerator_type: row.accelerator_type,
            accelerator_count: row.accelerator_count,
            container_port: row.container_port,
            env: row.env.0,
            args: row.args,
        }
    }
}

/// Launch history: everyone's, for an admin; only your own, for a `user`
/// account — same visibility split as the Pods tab and `auth::list_sessions`.
pub async fn list_launches(user: CurrentUser, State(state): State<AppState>) -> Result<Json<Vec<LaunchLogEntry>>, ApiError> {
    let rows: Vec<LaunchLogRow> = if user.role == Role::Admin {
        sqlx::query_as(
            "SELECT l.owner_username AS username, l.created_at::text, l.deployment_name, l.template_name, \
                 l.image, l.replicas, l.cpu_request, l.cpu_limit, l.memory_request, l.memory_limit, \
                 l.accelerator_type, l.accelerator_count, l.container_port, l.env, l.args \
             FROM launch_log l ORDER BY l.created_at DESC LIMIT 200",
        )
        .fetch_all(&state.pg)
        .await?
    } else {
        sqlx::query_as(
            "SELECT l.owner_username AS username, l.created_at::text, l.deployment_name, l.template_name, \
                 l.image, l.replicas, l.cpu_request, l.cpu_limit, l.memory_request, l.memory_limit, \
                 l.accelerator_type, l.accelerator_count, l.container_port, l.env, l.args \
             FROM launch_log l WHERE l.owner_username = $1 ORDER BY l.created_at DESC LIMIT 200",
        )
        .bind(&user.username)
        .fetch_all(&state.pg)
        .await?
    };
    Ok(Json(rows.into_iter().map(LaunchLogEntry::from).collect()))
}

/// The auto-generated secret's `(env_key, secret_value)` for a deployment, if
/// its template generated one — looked up fresh rather than trusted from the
/// request, since the actual value only ever lives in Postgres/the container.
async fn generated_secret(pg: &sqlx::PgPool, deployment_name: &str) -> Result<Option<(String, String)>, ApiError> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT env_key, secret_value FROM deployment_secrets WHERE deployment_name = $1")
            .bind(deployment_name)
            .fetch_optional(pg)
            .await?;
    Ok(row.and_then(|(key, value)| key.zip(value)))
}

/// Current editable state of a Deployment the caller owns (or, for an admin,
/// any Deployment) — backs the Pods tab's manage panel.
pub async fn get_deployment(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<DeploymentDetail>, ApiError> {
    let deployments: Api<Deployment> = Api::namespaced(state.client.clone(), &state.namespace);
    let deployment = deployments.get(&name).await?;
    check_owner(&deployment, &user)?;

    let secret = generated_secret(&state.pg, &name).await?;
    let secret_key = secret.as_ref().map(|(key, _)| key.clone());

    let container = deployment
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first());
    let resources = container.and_then(|c| c.resources.as_ref());
    let quantity = |map: Option<&BTreeMap<String, Quantity>>, key: &str| {
        map.and_then(|m| m.get(key)).map(|q| q.0.clone())
    };

    let env: Vec<(String, String)> = container
        .and_then(|c| c.env.as_ref())
        .map(|vars| {
            vars.iter()
                .filter(|v| Some(&v.name) != secret_key.as_ref())
                .map(|v| (v.name.clone(), v.value.clone().unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(DeploymentDetail {
        name,
        replicas: deployment.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
        cpu_request: quantity(resources.and_then(|r| r.requests.as_ref()), "cpu"),
        cpu_limit: quantity(resources.and_then(|r| r.limits.as_ref()), "cpu"),
        memory_request: quantity(resources.and_then(|r| r.requests.as_ref()), "memory"),
        memory_limit: quantity(resources.and_then(|r| r.limits.as_ref()), "memory"),
        env,
        generated_secret_key: secret_key,
    }))
}

/// Scales and/or updates resources/env on a Deployment the caller owns (or,
/// for an admin, any Deployment). Image, container port, accelerator, and
/// args are fixed at launch time — this only ever touches `spec.replicas`
/// and the first container's `resources`/`env`.
pub async fn update_deployment(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateDeploymentRequest>,
) -> Result<Json<DeploymentDetail>, ApiError> {
    let mut req = req;
    req.cpu_request = normalize_quantity(req.cpu_request.take());
    req.cpu_limit = normalize_quantity(req.cpu_limit.take());
    req.memory_request = normalize_quantity(req.memory_request.take());
    req.memory_limit = normalize_quantity(req.memory_limit.take());

    if req.replicas < 0 {
        return Err(ApiError::BadRequest("replicas must not be negative".into()));
    }
    for field in [
        ("cpu_request", &req.cpu_request),
        ("cpu_limit", &req.cpu_limit),
        ("memory_request", &req.memory_request),
        ("memory_limit", &req.memory_limit),
    ] {
        if let (name, Some(value)) = field {
            validate::quantity(name, value)?;
        }
    }
    for (key, _) in &req.env {
        if !key.trim().is_empty() {
            validate::env_key(key)?;
        }
    }
    validate::bounded_list("env", &req.env.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(), 50, 4096)?;

    let deployments: Api<Deployment> = Api::namespaced(state.client.clone(), &state.namespace);
    let mut deployment = deployments.get(&name).await?;
    check_owner(&deployment, &user)?;

    // Accelerators aren't part of UpdateDeploymentRequest (not editable
    // here), but they're preserved from the deployment's existing
    // resources below and still count toward the GPU quota - read them
    // before mutation to include in the check.
    let per_replica_gpu: i64 = deployment
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.resources.as_ref())
        .and_then(|r| r.limits.as_ref())
        .map(|limits| {
            limits.iter().filter(|(k, _)| k.as_str() != "cpu" && k.as_str() != "memory").filter_map(|(_, q)| parse_count(q)).sum()
        })
        .unwrap_or(0);

    let replicas = i64::from(req.replicas);
    let additional_cpu = req.cpu_limit.as_deref().and_then(|v| parse_cpu_millicores(&Quantity(v.to_string()))).unwrap_or(0) * replicas;
    let additional_memory = req.memory_limit.as_deref().and_then(|v| parse_memory_bytes(&Quantity(v.to_string()))).unwrap_or(0) * replicas;
    let additional_gpu = per_replica_gpu * replicas;
    let launch_tx = state.lock_launches().await?;
    quota::check_quota(&state, &user, Some(&name), additional_cpu, additional_memory, additional_gpu).await?;

    // Never regenerate an existing secret on edit - that would silently
    // invalidate a value a client may already be using. Just carry the
    // current one through untouched.
    let secret = generated_secret(&state.pg, &name).await?;

    // Same fixed-request substitution as create_deployment - see there.
    let global_settings = quota::load_global_settings(&state.pg).await?;
    let (cpu_request, memory_request) = if global_settings.expose_resource_requests {
        (req.cpu_request.clone(), req.memory_request.clone())
    } else {
        (global_settings.fixed_cpu_request.clone(), global_settings.fixed_memory_request.clone())
    };

    let mut requests = BTreeMap::new();
    let mut limits = BTreeMap::new();
    if let Some(v) = &cpu_request {
        requests.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.cpu_limit {
        limits.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &memory_request {
        requests.insert("memory".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.memory_limit {
        limits.insert("memory".to_string(), Quantity(v.clone()));
    }
    // Extended resources (accelerators) on the existing container, if any,
    // are preserved below by starting from its current resources rather
    // than building requests/limits from scratch.
    if let Some(container) =
        deployment.spec.as_mut().and_then(|s| s.template.spec.as_mut()).and_then(|s| s.containers.first())
        && let Some(existing) = &container.resources
    {
        for (map, existing_map) in [(&mut requests, &existing.requests), (&mut limits, &existing.limits)] {
            if let Some(existing_map) = existing_map {
                for (key, qty) in existing_map {
                    if key != "cpu" && key != "memory" {
                        map.entry(key.clone()).or_insert_with(|| qty.clone());
                    }
                }
            }
        }
    }

    let mut env: Vec<EnvVar> = req
        .env
        .iter()
        .filter(|(key, value)| !value.trim().is_empty() && Some(key.as_str()) != secret.as_ref().map(|(k, _)| k.as_str()))
        .map(|(name, value)| EnvVar {
            name: name.clone(),
            value: Some(value.clone()),
            ..Default::default()
        })
        .collect();
    if let Some((key, value)) = &secret {
        env.push(EnvVar { name: key.clone(), value: Some(value.clone()), ..Default::default() });
    }

    if let Some(spec) = deployment.spec.as_mut() {
        spec.replicas = Some(req.replicas);
        if let Some(container) = spec.template.spec.as_mut().and_then(|s| s.containers.first_mut()) {
            container.resources = Some(ResourceRequirements {
                requests: (!requests.is_empty()).then_some(requests),
                limits: (!limits.is_empty()).then_some(limits),
                ..Default::default()
            });
            container.env = (!env.is_empty()).then_some(env);
        }
    }

    let updated = deployments.replace(&name, &PostParams::default(), &deployment).await?;
    launch_tx.commit().await?;
    let secret_key = secret.map(|(key, _)| key);
    let out_container = updated.spec.as_ref().and_then(|s| s.template.spec.as_ref()).and_then(|s| s.containers.first());
    let out_env: Vec<(String, String)> = out_container
        .and_then(|c| c.env.as_ref())
        .map(|vars| {
            vars.iter()
                .filter(|v| Some(&v.name) != secret_key.as_ref())
                .map(|v| (v.name.clone(), v.value.clone().unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(DeploymentDetail {
        name,
        replicas: updated.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
        cpu_request: req.cpu_request,
        cpu_limit: req.cpu_limit,
        memory_request: req.memory_request,
        memory_limit: req.memory_limit,
        env: out_env,
        generated_secret_key: secret_key,
    }))
}

/// Deletes a Deployment the caller owns (or, for an admin, any Deployment),
/// along with its Service (if any) and stored credential/proxy metadata.
pub async fn delete_deployment(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<(), ApiError> {
    let deployments: Api<Deployment> = Api::namespaced(state.client.clone(), &state.namespace);
    let deployment = deployments.get(&name).await?;
    check_owner(&deployment, &user)?;

    deployments.delete(&name, &DeleteParams::default()).await?;

    let services: Api<Service> = Api::namespaced(state.client.clone(), &state.namespace);
    if let Err(err) = services.delete(&name, &DeleteParams::default()).await {
        // Not every deployment has a matching Service (no container_port).
        if !matches!(&err, kube::Error::Api(ae) if ae.code == 404) {
            return Err(err.into());
        }
    }

    sqlx::query("DELETE FROM deployment_secrets WHERE deployment_name = $1").bind(&name).execute(&state.pg).await?;
    Ok(())
}

/// Existing PersistentVolumeClaims in the watched namespace, for the
/// Launch/Templates forms' storage-mount fields. Any logged-in user (same
/// visibility level as the Images catalog) — this only lists claims that
/// already exist; Aether never creates or deletes one.
pub async fn list_pvcs(_user: CurrentUser, State(state): State<AppState>) -> Result<Json<Vec<PvcEntry>>, ApiError> {
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(state.client.clone(), &state.namespace);
    let list = pvcs.list(&ListParams::default()).await?;
    let entries: Vec<PvcEntry> = list
        .items
        .into_iter()
        .filter_map(|pvc| {
            let name = pvc.metadata.name?;
            let capacity =
                pvc.status.and_then(|s| s.capacity).and_then(|c| c.get("storage").cloned()).map(|Quantity(v)| v);
            Some(PvcEntry { name, capacity })
        })
        .collect();
    Ok(Json(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_collapses_and_trims_non_alphanumerics() {
        assert_eq!(slugify("JupyterLab"), "jupyterlab");
        assert_eq!(slugify("vLLM"), "vllm");
        assert_eq!(slugify("My Cool App!!"), "my-cool-app");
        assert_eq!(slugify("--leading-and-trailing--"), "leading-and-trailing");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn image_repo_slug_strips_registry_org_tag_and_digest() {
        assert_eq!(image_repo_slug("nginx:alpine"), "nginx");
        assert_eq!(image_repo_slug("jupyter/base-notebook"), "base-notebook");
        assert_eq!(image_repo_slug("ctr.int.example.com:8443/aether/aether:v1"), "aether");
        assert_eq!(image_repo_slug("gcr.io/distroless/cc-debian12:nonroot"), "cc-debian12");
        assert_eq!(image_repo_slug("nginx@sha256:abcd1234"), "nginx");
        // A pathological image string that slugifies to nothing still
        // produces a valid, non-empty instance type.
        assert_eq!(image_repo_slug("---"), "app");
    }

    #[test]
    fn scoped_deployment_name_has_the_right_shape() {
        let name = scoped_deployment_name("alice", "jupyterlab");
        assert!(name.starts_with("alice-jupyterlab-"), "{name}");
        assert_eq!(name.len(), "alice-jupyterlab-".len() + 6);
        // Different calls get different random suffixes.
        let other = scoped_deployment_name("alice", "jupyterlab");
        assert_ne!(name, other);
    }

    #[test]
    fn scoped_deployment_name_truncates_instance_type_to_fit_63_chars() {
        let username = "a".repeat(32);
        let instance_type = "b".repeat(100);
        let name = scoped_deployment_name(&username, &instance_type);
        assert!(name.len() <= 63, "{} chars: {name}", name.len());
        assert!(name.starts_with(&format!("{username}-")));
    }
}
