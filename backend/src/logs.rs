use axum::extract::{Path, Query, State};
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, LogParams};
use serde::Deserialize;

use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(Deserialize)]
pub struct LogsQuery {
    /// Defaults to the pod's only container, if it has just one.
    container: Option<String>,
    tail_lines: Option<i64>,
    /// Logs from the previous instance of the container, for one that crashed and restarted.
    #[serde(default)]
    previous: bool,
}

pub async fn get_pod_logs(
    _user: CurrentUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<LogsQuery>,
) -> Result<String, ApiError> {
    validate::k8s_name("name", &name)?;
    let api: Api<Pod> = Api::namespaced(state.client.clone(), &state.namespace);
    let params = LogParams {
        container: query.container,
        tail_lines: Some(query.tail_lines.unwrap_or(500)),
        previous: query.previous,
        timestamps: true,
        ..Default::default()
    };
    let logs = api.logs(&name, &params).await?;
    Ok(logs)
}
