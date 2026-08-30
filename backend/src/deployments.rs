use std::collections::BTreeMap;

use axum::extract::State;
use axum::Json;
use common::{CreateDeploymentRequest, CreateDeploymentResponse, LaunchLogEntry, Role};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements, Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, PostParams};
use sqlx::types::Json as SqlxJson;
use sqlx::FromRow;

use crate::auth::{generate_token, CurrentUser};
use crate::error::ApiError;
use crate::resources::OWNER_LABEL;
use crate::state::AppState;
use crate::validate;

pub async fn create_deployment(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(req): Json<CreateDeploymentRequest>,
) -> Result<Json<CreateDeploymentResponse>, ApiError> {
    validate::k8s_name("name", &req.name)?;
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

    let mut requests = BTreeMap::new();
    let mut limits = BTreeMap::new();
    if let Some(v) = &req.cpu_request {
        requests.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.cpu_limit {
        limits.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.memory_request {
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
    // "{{name}}" is a generic placeholder any template's args can use to refer
    // to the deployment's own (user-chosen) name, e.g. JupyterLab's
    // `--ServerApp.base_url=/proxy/{{name}}/`.
    let args: Vec<String> = req
        .args
        .iter()
        .map(|a| a.trim().replace("{{name}}", &req.name))
        .filter(|a| !a.is_empty())
        .collect();
    // `args` gets moved into the Container below; keep a copy for launch_log.
    let logged_args = args.clone();

    // The selector must stay a fixed, minimal set of labels the pod template
    // always carries; `owner` rides along as an extra descriptive label on
    // top of it (on the Deployment, its pods, and the Service), not part of
    // the selector itself.
    let mut selector_labels = BTreeMap::new();
    selector_labels.insert("app".to_string(), req.name.clone());
    let mut object_labels = selector_labels.clone();
    object_labels.insert(OWNER_LABEL.to_string(), user.username.clone());

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(req.name.clone()),
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
                    containers: vec![Container {
                        name: req.name.clone(),
                        image: Some(req.image.clone()),
                        resources: Some(ResourceRequirements {
                            requests: (!requests.is_empty()).then_some(requests),
                            limits: (!limits.is_empty()).then_some(limits),
                            ..Default::default()
                        }),
                        env: (!env.is_empty()).then_some(env),
                        args: (!args.is_empty()).then_some(args),
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
    let created = deployments.create(&PostParams::default(), &deployment).await?;
    let name = created.metadata.name.unwrap_or(req.name);

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

    let proxy_path = req.enable_proxy.then(|| format!("/proxy/{name}/"));

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
