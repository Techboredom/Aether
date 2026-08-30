use axum::extract::{Path, State};
use axum::Json;
use common::{ImageEntry, SaveImageRequest};
use sqlx::FromRow;

use crate::auth::{AdminUser, CurrentUser};
use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(FromRow)]
struct ImageRow {
    id: i32,
    name: String,
    image: String,
    description: String,
}

impl From<ImageRow> for ImageEntry {
    fn from(row: ImageRow) -> Self {
        ImageEntry {
            id: row.id,
            name: row.name,
            image: row.image,
            description: row.description,
        }
    }
}

pub async fn list_images(_user: CurrentUser, State(state): State<AppState>) -> Result<Json<Vec<ImageEntry>>, ApiError> {
    let rows: Vec<ImageRow> = sqlx::query_as("SELECT id, name, image, description FROM images ORDER BY name")
        .fetch_all(&state.pg)
        .await?;
    Ok(Json(rows.into_iter().map(ImageEntry::from).collect()))
}

pub async fn create_image(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<SaveImageRequest>,
) -> Result<Json<ImageEntry>, ApiError> {
    validate_request(&req)?;
    let row: ImageRow = sqlx::query_as(
        "INSERT INTO images (name, image, description) VALUES ($1, $2, $3) \
         RETURNING id, name, image, description",
    )
    .bind(&req.name)
    .bind(&req.image)
    .bind(&req.description)
    .fetch_one(&state.pg)
    .await?;
    Ok(Json(row.into()))
}

pub async fn update_image(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<SaveImageRequest>,
) -> Result<Json<ImageEntry>, ApiError> {
    validate_request(&req)?;
    let row: Option<ImageRow> = sqlx::query_as(
        "UPDATE images SET name = $1, image = $2, description = $3 WHERE id = $4 \
         RETURNING id, name, image, description",
    )
    .bind(&req.name)
    .bind(&req.image)
    .bind(&req.description)
    .bind(id)
    .fetch_optional(&state.pg)
    .await?;

    let row = row.ok_or_else(|| ApiError::BadRequest(format!("image {id} not found")))?;
    Ok(Json(row.into()))
}

pub async fn delete_image(_admin: AdminUser, State(state): State<AppState>, Path(id): Path<i32>) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM images WHERE id = $1").bind(id).execute(&state.pg).await?;
    Ok(())
}

fn validate_request(req: &SaveImageRequest) -> Result<(), ApiError> {
    validate::label("name", &req.name, 100)?;
    validate::image_ref(&req.image)?;
    if req.description.chars().count() > 500 {
        return Err(ApiError::BadRequest("description: must be at most 500 characters".to_string()));
    }
    Ok(())
}
