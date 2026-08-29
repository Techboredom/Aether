use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use common::{LoginRequest, Role, UserInfo};
use rand::distr::Alphanumeric;
use rand::RngExt;
use sqlx::FromRow;
use time::Duration;

use crate::error::ApiError;
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "aether_session";
const SESSION_LIFETIME_DAYS: i64 = 7;

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|err| ApiError::BadRequest(format!("failed to hash password: {err}")))
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else { return false };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// A high-entropy random token, used both for session cookies and for
/// auto-generated app credentials (JupyterLab tokens, RStudio passwords, ...).
pub fn generate_token() -> String {
    rand::rng().sample_iter(Alphanumeric).take(48).map(char::from).collect()
}

/// The authenticated caller, extracted from the `aether_session` cookie.
/// Any endpoint that takes this as a parameter requires a logged-in user of
/// either role — use `AdminUser` instead for admin-only endpoints.
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i32,
    pub username: String,
    pub role: Role,
}

#[derive(FromRow)]
struct SessionUserRow {
    id: i32,
    username: String,
    role: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state).await.expect("infallible");
        let token = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()).ok_or(ApiError::Unauthorized)?;

        let row: Option<SessionUserRow> = sqlx::query_as(
            "SELECT u.id, u.username, u.role FROM sessions s \
             JOIN users u ON u.id = s.user_id \
             WHERE s.token = $1 AND s.expires_at > now()",
        )
        .bind(&token)
        .fetch_optional(&state.pg)
        .await
        .map_err(ApiError::from)?;

        let row = row.ok_or(ApiError::Unauthorized)?;
        let role = if row.role == "admin" { Role::Admin } else { Role::User };
        Ok(CurrentUser { id: row.id, username: row.username, role })
    }
}

/// Same as `CurrentUser`, but rejects non-admins with 403. Use this as the
/// parameter type for admin-only endpoints (Templates writes, Users management).
pub struct AdminUser(pub CurrentUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        if user.role != Role::Admin {
            return Err(ApiError::Forbidden("admin role required".to_string()));
        }
        Ok(AdminUser(user))
    }
}

#[derive(FromRow)]
struct UserAuthRow {
    id: i32,
    username: String,
    password_hash: String,
    role: String,
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<(CookieJar, Json<UserInfo>), ApiError> {
    let row: Option<UserAuthRow> = sqlx::query_as("SELECT id, username, password_hash, role FROM users WHERE username = $1")
        .bind(&req.username)
        .fetch_optional(&state.pg)
        .await?;

    let row = row.filter(|r| verify_password(&r.password_hash, &req.password));
    let Some(row) = row else {
        return Err(ApiError::Unauthorized);
    };

    let token = generate_token();
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, now() + make_interval(days => $3))")
        .bind(&token)
        .bind(row.id)
        .bind(SESSION_LIFETIME_DAYS as i32)
        .execute(&state.pg)
        .await?;

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(Duration::days(SESSION_LIFETIME_DAYS))
        .build();

    let role = if row.role == "admin" { Role::Admin } else { Role::User };
    Ok((jar.add(cookie), Json(UserInfo { id: row.id, username: row.username, role })))
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<CookieJar, ApiError> {
    if let Some(token) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) {
        sqlx::query("DELETE FROM sessions WHERE token = $1").bind(&token).execute(&state.pg).await?;
    }
    // Must match the `path` the cookie was set with (login's Set-Cookie), or the
    // browser treats this as a different cookie and never actually clears the session.
    Ok(jar.remove(Cookie::build((SESSION_COOKIE, "")).path("/").build()))
}

pub async fn me(user: CurrentUser) -> Json<UserInfo> {
    Json(UserInfo { id: user.id, username: user.username, role: user.role })
}
