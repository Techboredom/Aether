use axum::extract::{Path, State};
use axum::Json;
use common::{SaveTemplateRequest, TemplateEntry};
use sqlx::types::Json as SqlxJson;
use sqlx::{AssertSqlSafe, FromRow};

use crate::error::ApiError;
use crate::state::AppState;

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
    notes: String,
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
            notes: row.notes,
        }
    }
}

// `SELECT_COLUMNS` is a compile-time constant, never user input, so interpolating it
// into these queries with `AssertSqlSafe` below is not a SQL-injection risk.
const SELECT_COLUMNS: &str = "id, name, image, container_port, cpu_request, cpu_limit, memory_request, \
     memory_limit, accelerator_type, accelerator_count, env, args, notes";

pub async fn list_templates(State(state): State<AppState>) -> Result<Json<Vec<TemplateEntry>>, ApiError> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM templates ORDER BY name");
    let rows: Vec<TemplateRow> = sqlx::query_as(AssertSqlSafe(sql)).fetch_all(&state.pg).await?;
    Ok(Json(rows.into_iter().map(TemplateEntry::from).collect()))
}

pub async fn create_template(
    State(state): State<AppState>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<TemplateEntry>, ApiError> {
    validate(&req)?;
    let sql = format!(
        "INSERT INTO templates (name, image, container_port, cpu_request, cpu_limit, memory_request, \
         memory_limit, accelerator_type, accelerator_count, env, args, notes) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
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
        .bind(&req.notes)
        .fetch_one(&state.pg)
        .await?;
    Ok(Json(row.into()))
}

pub async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<TemplateEntry>, ApiError> {
    validate(&req)?;
    let sql = format!(
        "UPDATE templates SET name = $1, image = $2, container_port = $3, cpu_request = $4, cpu_limit = $5, \
         memory_request = $6, memory_limit = $7, accelerator_type = $8, accelerator_count = $9, env = $10, \
         args = $11, notes = $12 \
         WHERE id = $13 \
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
        .bind(&req.notes)
        .bind(id)
        .fetch_optional(&state.pg)
        .await?;

    let row = row.ok_or_else(|| ApiError::BadRequest(format!("template {id} not found")))?;
    Ok(Json(row.into()))
}

pub async fn delete_template(State(state): State<AppState>, Path(id): Path<i32>) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM templates WHERE id = $1").bind(id).execute(&state.pg).await?;
    Ok(())
}

fn validate(req: &SaveTemplateRequest) -> Result<(), ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if req.image.trim().is_empty() {
        return Err(ApiError::BadRequest("image is required".into()));
    }
    Ok(())
}
