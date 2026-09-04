use axum::extract::{Path, State};
use axum::Json;
use common::{SaveTemplateRequest, TemplateEntry};
use sqlx::types::Json as SqlxJson;
use sqlx::{AssertSqlSafe, FromRow};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(FromRow)]
struct TemplateRow {
    id: i32,
    name: String,
    image: String,
    container_port: Option<i32>,
    cpu_request: String,
    cpu_limit: String,
    memory_request: String,
    memory_limit: String,
    accelerator_type: String,
    accelerator_count: Option<i64>,
    env: SqlxJson<Vec<(String, String)>>,
    args: Vec<String>,
    model: String,
    volume_claim_name: String,
    volume_mount_path: String,
    volume_sub_path: String,
    notes: String,
    secret_env_key: Option<String>,
    proxy_enabled: bool,
    strip_prefix: bool,
    public_service: bool,
}

impl From<TemplateRow> for TemplateEntry {
    fn from(row: TemplateRow) -> Self {
        TemplateEntry {
            id: row.id,
            name: row.name,
            image: row.image,
            container_port: row.container_port,
            cpu_request: row.cpu_request,
            cpu_limit: row.cpu_limit,
            memory_request: row.memory_request,
            memory_limit: row.memory_limit,
            accelerator_type: row.accelerator_type,
            accelerator_count: row.accelerator_count,
            env: row.env.0,
            args: row.args,
            model: row.model,
            volume_claim_name: row.volume_claim_name,
            volume_mount_path: row.volume_mount_path,
            volume_sub_path: row.volume_sub_path,
            notes: row.notes,
            secret_env_key: row.secret_env_key,
            proxy_enabled: row.proxy_enabled,
            strip_prefix: row.strip_prefix,
            public_service: row.public_service,
        }
    }
}

// `SELECT_COLUMNS` is a compile-time constant, never user input, so interpolating it
// into these queries with `AssertSqlSafe` below is not a SQL-injection risk.
const SELECT_COLUMNS: &str = "id, name, image, container_port, cpu_request, cpu_limit, memory_request, \
     memory_limit, accelerator_type, accelerator_count, env, args, model, volume_claim_name, \
     volume_mount_path, volume_sub_path, notes, secret_env_key, proxy_enabled, strip_prefix, public_service";

pub async fn list_templates(
    _user: CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TemplateEntry>>, ApiError> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM templates ORDER BY name");
    let rows: Vec<TemplateRow> = sqlx::query_as(AssertSqlSafe(sql)).fetch_all(&state.pg).await?;
    Ok(Json(rows.into_iter().map(TemplateEntry::from).collect()))
}

pub async fn create_template(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<TemplateEntry>, ApiError> {
    validate_request(&req)?;
    let sql = format!(
        "INSERT INTO templates (name, image, container_port, cpu_request, cpu_limit, memory_request, \
         memory_limit, accelerator_type, accelerator_count, env, args, model, volume_claim_name, \
         volume_mount_path, volume_sub_path, notes, secret_env_key, proxy_enabled, strip_prefix, public_service) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20) \
         RETURNING {SELECT_COLUMNS}"
    );
    let row: TemplateRow = sqlx::query_as(AssertSqlSafe(sql))
        .bind(&req.name)
        .bind(&req.image)
        .bind(req.container_port)
        .bind(&req.cpu_request)
        .bind(&req.cpu_limit)
        .bind(&req.memory_request)
        .bind(&req.memory_limit)
        .bind(&req.accelerator_type)
        .bind(req.accelerator_count)
        .bind(SqlxJson(&req.env))
        .bind(&req.args)
        .bind(&req.model)
        .bind(&req.volume_claim_name)
        .bind(&req.volume_mount_path)
        .bind(&req.volume_sub_path)
        .bind(&req.notes)
        .bind(&req.secret_env_key)
        .bind(req.proxy_enabled)
        .bind(req.strip_prefix)
        .bind(req.public_service)
        .fetch_one(&state.pg)
        .await?;
    Ok(Json(row.into()))
}

pub async fn update_template(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<TemplateEntry>, ApiError> {
    validate_request(&req)?;
    let sql = format!(
        "UPDATE templates SET name = $1, image = $2, container_port = $3, cpu_request = $4, cpu_limit = $5, \
         memory_request = $6, memory_limit = $7, accelerator_type = $8, accelerator_count = $9, env = $10, \
         args = $11, model = $12, volume_claim_name = $13, volume_mount_path = $14, volume_sub_path = $15, \
         notes = $16, secret_env_key = $17, proxy_enabled = $18, strip_prefix = $19, public_service = $20 \
         WHERE id = $21 \
         RETURNING {SELECT_COLUMNS}"
    );
    let row: Option<TemplateRow> = sqlx::query_as(AssertSqlSafe(sql))
        .bind(&req.name)
        .bind(&req.image)
        .bind(req.container_port)
        .bind(&req.cpu_request)
        .bind(&req.cpu_limit)
        .bind(&req.memory_request)
        .bind(&req.memory_limit)
        .bind(&req.accelerator_type)
        .bind(req.accelerator_count)
        .bind(SqlxJson(&req.env))
        .bind(&req.args)
        .bind(&req.model)
        .bind(&req.volume_claim_name)
        .bind(&req.volume_mount_path)
        .bind(&req.volume_sub_path)
        .bind(&req.notes)
        .bind(&req.secret_env_key)
        .bind(req.proxy_enabled)
        .bind(req.strip_prefix)
        .bind(req.public_service)
        .bind(id)
        .fetch_optional(&state.pg)
        .await?;

    let row = row.ok_or_else(|| ApiError::BadRequest(format!("template {id} not found")))?;
    Ok(Json(row.into()))
}

pub async fn delete_template(_admin: AdminUser, State(state): State<AppState>, Path(id): Path<i32>) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM templates WHERE id = $1").bind(id).execute(&state.pg).await?;
    Ok(())
}

fn validate_request(req: &SaveTemplateRequest) -> Result<(), ApiError> {
    validate::label("name", &req.name, 100)?;
    validate::image_ref(&req.image)?;
    if let Some(port) = req.container_port {
        validate::container_port(port)?;
    }
    for (field, value) in [
        ("cpu_request", &req.cpu_request),
        ("cpu_limit", &req.cpu_limit),
        ("memory_request", &req.memory_request),
        ("memory_limit", &req.memory_limit),
    ] {
        validate::quantity(field, value)?;
    }
    for (key, _) in &req.env {
        if !key.trim().is_empty() {
            validate::env_key(key)?;
        }
    }
    validate::bounded_list("env", &req.env.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(), 50, 4096)?;
    validate::bounded_list("args", &req.args, 50, 1024)?;
    validate::optional_text("model", &req.model, 500)?;
    validate::volume_mount(&req.volume_claim_name, &req.volume_mount_path)?;
    if req.notes.chars().count() > 2000 {
        return Err(ApiError::BadRequest("notes: must be at most 2000 characters".to_string()));
    }
    if let Some(key) = &req.secret_env_key {
        validate::env_key(key)?;
    }
    Ok(())
}
