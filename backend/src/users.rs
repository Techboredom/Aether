use axum::extract::{Path, State};
use axum::Json;
use common::{CreateUserRequest, ResetPasswordRequest, Role, UserInfo};
use sqlx::FromRow;

use crate::auth::{hash_password, AdminUser};
use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(FromRow)]
struct UserRow {
    id: i32,
    username: String,
    role: String,
}

impl From<UserRow> for UserInfo {
    fn from(row: UserRow) -> Self {
        UserInfo { id: row.id, username: row.username, role: if row.role == "admin" { Role::Admin } else { Role::User } }
    }
}

pub async fn list_users(_admin: AdminUser, State(state): State<AppState>) -> Result<Json<Vec<UserInfo>>, ApiError> {
    let rows: Vec<UserRow> = sqlx::query_as("SELECT id, username, role FROM users ORDER BY username").fetch_all(&state.pg).await?;
    Ok(Json(rows.into_iter().map(UserInfo::from).collect()))
}

pub async fn create_user(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<UserInfo>, ApiError> {
    validate::username(&req.username)?;
    validate::password(&req.password)?;

    let password_hash = hash_password(&req.password)?;
    let role_str = if req.role == Role::Admin { "admin" } else { "user" };

    let row: UserRow = sqlx::query_as("INSERT INTO users (username, password_hash, role) VALUES ($1, $2, $3) RETURNING id, username, role")
        .bind(&req.username)
        .bind(&password_hash)
        .bind(role_str)
        .fetch_one(&state.pg)
        .await
        .map_err(|err| match err {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                ApiError::BadRequest(format!("username \"{}\" is already taken", req.username))
            }
            other => ApiError::from(other),
        })?;

    Ok(Json(row.into()))
}

pub async fn delete_user(admin: AdminUser, State(state): State<AppState>, Path(id): Path<i32>) -> Result<(), ApiError> {
    if admin.0.id == id {
        return Err(ApiError::BadRequest("you can't delete your own account".to_string()));
    }
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(&state.pg).await?;
    Ok(())
}

/// An admin can reset any account's password without knowing the old one —
/// the admin role itself is the authorization. All of that account's
/// sessions are invalidated, forcing a fresh login with the new password.
pub async fn reset_password(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<(), ApiError> {
    validate::password(&req.password)?;
    let password_hash = hash_password(&req.password)?;
    let result = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&password_hash)
        .bind(id)
        .execute(&state.pg)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::BadRequest(format!("user {id} not found")));
    }
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(&state.pg).await?;
    Ok(())
}
