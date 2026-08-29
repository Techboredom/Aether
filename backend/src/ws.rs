use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use common::PodEvent;

use crate::auth::CurrentUser;
use crate::state::AppState;
use crate::visibility;

pub async fn list_pods(user: CurrentUser, State(state): State<AppState>) -> Json<Vec<common::PodInfo>> {
    let pods = state.snapshot().await;
    Json(visibility::visible_to(pods, &user, &state.pg).await)
}

pub async fn ws_handler(user: CurrentUser, ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, user))
}

async fn handle_socket(mut socket: WebSocket, state: AppState, user: CurrentUser) {
    // Subscribe before reading the snapshot so no update can be missed in between;
    // any event replayed for a pod already in the snapshot is a harmless no-op overwrite.
    let mut rx = state.subscribe();
    let snapshot = visibility::visible_to(state.snapshot().await, &user, &state.pg).await;

    if send_event(&mut socket, &PodEvent::Snapshot { pods: snapshot }).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(PodEvent::Upsert { mut pod }) => {
                        if !visibility::can_see(&pod, &user) {
                            continue;
                        }
                        visibility::attach_credential(&mut pod, &state.pg).await;
                        if send_event(&mut socket, &PodEvent::Upsert { pod }).await.is_err() {
                            break;
                        }
                    }
                    // A pod's name reveals nothing sensitive on its own, and a client
                    // that was never shown it (filtered out above) just no-ops removing
                    // an unknown key — no need to check ownership before forwarding this.
                    Ok(event @ PodEvent::Delete { .. }) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Ok(PodEvent::Snapshot { .. }) => {} // never actually broadcast; see state.rs
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Client fell behind; resync it with a fresh full snapshot.
                        let snapshot = visibility::visible_to(state.snapshot().await, &user, &state.pg).await;
                        if send_event(&mut socket, &PodEvent::Snapshot { pods: snapshot }).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn send_event(socket: &mut WebSocket, event: &PodEvent) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(event).expect("PodEvent always serializes");
    socket.send(Message::Text(payload.into())).await
}
