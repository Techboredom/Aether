use axum::extract::{Path, State};
use axum::Json;
use common::PodEventInfo;
use k8s_openapi::api::core::v1::Event;
use kube::api::{Api, ListParams};

use crate::error::ApiError;
use crate::state::AppState;

pub async fn get_pod_events(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<PodEventInfo>>, ApiError> {
    let api: Api<Event> = Api::namespaced(state.client.clone(), &state.namespace);
    let selector = format!("involvedObject.kind=Pod,involvedObject.name={name},involvedObject.namespace={}", state.namespace);
    let events = api.list(&ListParams::default().fields(&selector)).await?;

    let mut infos: Vec<PodEventInfo> = events
        .into_iter()
        .map(|event| PodEventInfo {
            type_: event.type_.unwrap_or_else(|| "Normal".to_string()),
            reason: event.reason.unwrap_or_default(),
            message: event.message.unwrap_or_default(),
            count: event.count.unwrap_or(1),
            last_seen: event.last_timestamp.map(|t| t.0.to_string()),
        })
        .collect();
    infos.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

    Ok(Json(infos))
}
