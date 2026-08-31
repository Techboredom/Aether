use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::Json;
use common::{MyQuota, PodInfo, QuotaLimits, QuotaSettings, Role, UserQuotaEntry};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use sqlx::PgPool;

use crate::auth::{AdminUser, CurrentUser};
use crate::error::ApiError;
use crate::resources::{parse_cpu_millicores, parse_memory_bytes};
use crate::state::AppState;
use crate::validate;

#[derive(Default)]
pub struct GlobalSettings {
    pub limits: QuotaLimits,
    pub expose_resource_requests: bool,
    pub fixed_cpu_request: Option<String>,
    pub fixed_memory_request: Option<String>,
}

type GlobalSettingsRow = (Option<String>, Option<String>, Option<i32>, bool, Option<String>, Option<String>);

pub async fn load_global_settings(pg: &PgPool) -> Result<GlobalSettings, ApiError> {
    let row: GlobalSettingsRow = sqlx::query_as(
        "SELECT cpu_limit, memory_limit, gpu_limit, expose_resource_requests, fixed_cpu_request, fixed_memory_request \
         FROM quota_settings WHERE id = 1",
    )
    .fetch_one(pg)
    .await?;
    Ok(GlobalSettings {
        limits: QuotaLimits { cpu_limit: row.0, memory_limit: row.1, gpu_limit: row.2 },
        expose_resource_requests: row.3,
        fixed_cpu_request: row.4,
        fixed_memory_request: row.5,
    })
}

async fn load_user_override(pg: &PgPool, user_id: i32) -> Result<Option<QuotaLimits>, ApiError> {
    let row: Option<(Option<String>, Option<String>, Option<i32>)> =
        sqlx::query_as("SELECT cpu_limit, memory_limit, gpu_limit FROM user_quotas WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pg)
            .await?;
    Ok(row.map(|(cpu_limit, memory_limit, gpu_limit)| QuotaLimits { cpu_limit, memory_limit, gpu_limit }))
}

/// `(limits, is_override, expose_resource_requests)` for `user` — an
/// admin's `limits` always come back unlimited, since admins are exempt
/// from enforcement, but `expose_resource_requests` is a UI setting that
/// still applies to everyone.
async fn effective_quota(state: &AppState, user: &CurrentUser) -> Result<(QuotaLimits, bool, bool), ApiError> {
    let global = load_global_settings(&state.pg).await?;
    if user.role == Role::Admin {
        return Ok((QuotaLimits::default(), false, global.expose_resource_requests));
    }
    match load_user_override(&state.pg, user.id).await? {
        Some(over) => Ok((over, true, global.expose_resource_requests)),
        None => Ok((global.limits, false, global.expose_resource_requests)),
    }
}

/// Sums limit-based CPU/memory/GPU usage across `pods` owned by `username`,
/// optionally excluding one deployment's pods — used when editing an
/// existing deployment, so its current footprint isn't counted twice
/// against the proposed new one.
fn usage_from_pods(pods: &[PodInfo], username: &str, exclude_deployment: Option<&str>) -> (i64, i64, i64) {
    let mut cpu = 0i64;
    let mut mem = 0i64;
    let mut gpu = 0i64;
    for pod in pods.iter().filter(|p| p.owner.as_deref() == Some(username)) {
        if exclude_deployment.is_some() && pod.deployment_name.as_deref() == exclude_deployment {
            continue;
        }
        cpu += pod.cpu_limit_millicores.unwrap_or(0);
        mem += pod.memory_limit_bytes.unwrap_or(0);
        gpu += pod.accelerators.values().sum::<i64>();
    }
    (cpu, mem, gpu)
}

fn format_gib(bytes: i64) -> String {
    format!("{:.2}Gi", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Checked by `deployments::create_deployment`/`update_deployment` before
/// touching the cluster. No-op for admins. `additional_*` is the
/// footprint (already multiplied by replicas) the caller is trying to add;
/// `exclude_deployment` should be the deployment being edited, if any, so
/// its own current usage isn't double-counted against the new proposal.
pub async fn check_quota(
    state: &AppState,
    user: &CurrentUser,
    exclude_deployment: Option<&str>,
    additional_cpu_millicores: i64,
    additional_memory_bytes: i64,
    additional_gpu_count: i64,
) -> Result<(), ApiError> {
    if user.role == Role::Admin {
        return Ok(());
    }
    let (limits, _, _) = effective_quota(state, user).await?;
    let pods = state.snapshot().await;
    let (used_cpu, used_mem, used_gpu) = usage_from_pods(&pods, &user.username, exclude_deployment);

    let total_cpu = used_cpu + additional_cpu_millicores;
    let total_mem = used_mem + additional_memory_bytes;
    let total_gpu = used_gpu + additional_gpu_count;

    if let Some(limit) = &limits.cpu_limit
        && let Some(limit_millicores) = parse_cpu_millicores(&Quantity(limit.clone()))
        && total_cpu > limit_millicores
    {
        return Err(ApiError::BadRequest(format!(
            "CPU limit quota exceeded: this would use {:.2} cores total, your limit is {limit} cores",
            total_cpu as f64 / 1000.0
        )));
    }
    if let Some(limit) = &limits.memory_limit
        && let Some(limit_bytes) = parse_memory_bytes(&Quantity(limit.clone()))
        && total_mem > limit_bytes
    {
        return Err(ApiError::BadRequest(format!(
            "Memory limit quota exceeded: this would use {} total, your limit is {limit}",
            format_gib(total_mem)
        )));
    }
    if let Some(limit) = limits.gpu_limit
        && total_gpu > i64::from(limit)
    {
        return Err(ApiError::BadRequest(format!(
            "GPU quota exceeded: this would use {total_gpu} total, your limit is {limit}"
        )));
    }
    Ok(())
}

fn validate_limits(limits: &QuotaLimits) -> Result<(), ApiError> {
    if let Some(v) = &limits.cpu_limit {
        validate::quantity("cpu_limit", v)?;
    }
    if let Some(v) = &limits.memory_limit {
        validate::quantity("memory_limit", v)?;
    }
    if let Some(v) = limits.gpu_limit
        && v < 0
    {
        return Err(ApiError::BadRequest("gpu_limit: must not be negative".to_string()));
    }
    Ok(())
}

/// The caller's own effective quota + current usage — backs the Launch tab
/// and the Pods tab's manage panel, both of which need
/// `expose_resource_requests` regardless of role.
pub async fn my_quota(user: CurrentUser, State(state): State<AppState>) -> Result<Json<MyQuota>, ApiError> {
    let (limits, is_override, expose_resource_requests) = effective_quota(&state, &user).await?;
    // Fixed requests are a global-only setting (not per-user), and purely
    // informational here — the frontend never sends them back.
    let global = load_global_settings(&state.pg).await?;
    let pods = state.snapshot().await;
    let (used_cpu, used_mem, used_gpu) = usage_from_pods(&pods, &user.username, None);
    Ok(Json(MyQuota {
        limits,
        is_override,
        expose_resource_requests,
        fixed_cpu_request: global.fixed_cpu_request,
        fixed_memory_request: global.fixed_memory_request,
        used_cpu_millicores: used_cpu,
        used_memory_bytes: used_mem,
        used_gpu_count: used_gpu,
    }))
}

pub async fn get_settings(_admin: AdminUser, State(state): State<AppState>) -> Result<Json<QuotaSettings>, ApiError> {
    let global = load_global_settings(&state.pg).await?;
    Ok(Json(QuotaSettings {
        limits: global.limits,
        expose_resource_requests: global.expose_resource_requests,
        fixed_cpu_request: global.fixed_cpu_request,
        fixed_memory_request: global.fixed_memory_request,
    }))
}

pub async fn update_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<QuotaSettings>,
) -> Result<Json<QuotaSettings>, ApiError> {
    validate_limits(&req.limits)?;
    if let Some(v) = &req.fixed_cpu_request {
        validate::quantity("fixed_cpu_request", v)?;
    }
    if let Some(v) = &req.fixed_memory_request {
        validate::quantity("fixed_memory_request", v)?;
    }
    sqlx::query(
        "UPDATE quota_settings SET cpu_limit = $1, memory_limit = $2, gpu_limit = $3, \
         expose_resource_requests = $4, fixed_cpu_request = $5, fixed_memory_request = $6 WHERE id = 1",
    )
    .bind(&req.limits.cpu_limit)
    .bind(&req.limits.memory_limit)
    .bind(req.limits.gpu_limit)
    .bind(req.expose_resource_requests)
    .bind(&req.fixed_cpu_request)
    .bind(&req.fixed_memory_request)
    .execute(&state.pg)
    .await?;
    Ok(Json(req))
}

pub async fn list_user_quotas(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserQuotaEntry>>, ApiError> {
    type OverrideRow = (i32, Option<String>, Option<String>, Option<i32>);

    let users: Vec<(i32, String)> = sqlx::query_as("SELECT id, username FROM users ORDER BY username").fetch_all(&state.pg).await?;
    let override_rows: Vec<OverrideRow> =
        sqlx::query_as("SELECT user_id, cpu_limit, memory_limit, gpu_limit FROM user_quotas").fetch_all(&state.pg).await?;
    let overrides: HashMap<i32, QuotaLimits> = override_rows
        .into_iter()
        .map(|(id, cpu_limit, memory_limit, gpu_limit)| (id, QuotaLimits { cpu_limit, memory_limit, gpu_limit }))
        .collect();

    let pods = state.snapshot().await;
    let entries = users
        .into_iter()
        .map(|(id, username)| {
            let (cpu, mem, gpu) = usage_from_pods(&pods, &username, None);
            UserQuotaEntry {
                quota_override: overrides.get(&id).cloned(),
                user_id: id,
                username,
                used_cpu_millicores: cpu,
                used_memory_bytes: mem,
                used_gpu_count: gpu,
            }
        })
        .collect();
    Ok(Json(entries))
}

pub async fn set_user_quota(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<QuotaLimits>,
) -> Result<Json<QuotaLimits>, ApiError> {
    validate_limits(&req)?;
    sqlx::query(
        "INSERT INTO user_quotas (user_id, cpu_limit, memory_limit, gpu_limit) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id) DO UPDATE SET \
            cpu_limit = EXCLUDED.cpu_limit, memory_limit = EXCLUDED.memory_limit, gpu_limit = EXCLUDED.gpu_limit",
    )
    .bind(id)
    .bind(&req.cpu_limit)
    .bind(&req.memory_limit)
    .bind(req.gpu_limit)
    .execute(&state.pg)
    .await?;
    Ok(Json(req))
}

pub async fn clear_user_quota(_admin: AdminUser, State(state): State<AppState>, Path(id): Path<i32>) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM user_quotas WHERE user_id = $1").bind(id).execute(&state.pg).await?;
    Ok(())
}
