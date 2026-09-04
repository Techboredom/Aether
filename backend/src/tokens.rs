use axum::extract::{Path, State};
use axum::Json;
use common::{ApiTokenCreated, ApiTokenEntry, CreateApiTokenRequest};
use sqlx::FromRow;

use crate::auth::{generate_api_token, hash_token, AdminUser};
use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(FromRow)]
struct ApiTokenRow {
    id: i32,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
}

impl From<ApiTokenRow> for ApiTokenEntry {
    fn from(row: ApiTokenRow) -> Self {
        ApiTokenEntry { id: row.id, name: row.name, created_at: row.created_at, last_used_at: row.last_used_at }
    }
}

/// Issues a new API token authenticating as the calling admin — an
/// alternate credential to a session cookie, meant for scripts/automation
/// rather than a browser (see README's "Admin API tokens" section). The
/// raw value is returned here and only here: only its SHA-256 hash is ever
/// stored (`api_tokens.token_hash`, see `auth::hash_token`), so there's no
/// way to recover it again later, by anyone, even an admin looking at
/// their own token's row.
pub async fn create_token(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<CreateApiTokenRequest>,
) -> Result<Json<ApiTokenCreated>, ApiError> {
    validate::label("name", &req.name, 100)?;
    let name = req.name.trim().to_string();
    let token = generate_api_token();
    let hash = hash_token(&token);

    let row: (i32, String) =
        sqlx::query_as("INSERT INTO api_tokens (user_id, name, token_hash) VALUES ($1, $2, $3) RETURNING id, created_at::text")
            .bind(admin.0.id)
            .bind(&name)
            .bind(&hash)
            .fetch_one(&state.pg)
            .await?;

    Ok(Json(ApiTokenCreated { id: row.0, name, token, created_at: row.1 }))
}

/// Lists the calling admin's own tokens — never another admin's tokens,
/// and never a raw value, only enough to tell them apart and see whether
/// one is still being used before revoking it.
pub async fn list_tokens(admin: AdminUser, State(state): State<AppState>) -> Result<Json<Vec<ApiTokenEntry>>, ApiError> {
    let rows: Vec<ApiTokenRow> = sqlx::query_as(
        "SELECT id, name, created_at::text, last_used_at::text FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(admin.0.id)
    .fetch_all(&state.pg)
    .await?;
    Ok(Json(rows.into_iter().map(ApiTokenEntry::from).collect()))
}

/// Revokes one of the calling admin's own tokens. 400 if it doesn't exist
/// or belongs to someone else — the same error either way, so this can't
/// be used to probe which token ids exist on other accounts.
pub async fn delete_token(admin: AdminUser, State(state): State<AppState>, Path(id): Path<i32>) -> Result<(), ApiError> {
    let result =
        sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2").bind(id).bind(admin.0.id).execute(&state.pg).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::BadRequest(format!("token {id} not found")));
    }
    Ok(())
}
