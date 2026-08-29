use axum::extract::State;
use axum::Json;
use common::ImageEntry;
use sqlx::FromRow;

use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;

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
