use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use common::{PodEvent, PodInfo};

use crate::auth::CurrentUser;
use crate::state::AppState;

pub async fn list_pods(_user: CurrentUser, State(state): State<AppState>) -> Json<Vec<PodInfo>> {
    Json(state.snapshot().await)
}

pub async fn ws_handler(_user: CurrentUser, ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // Subscribe before reading the snapshot so no update can be missed in between;
    // any event replayed for a pod already in the snapshot is a harmless no-op overwrite.
    let mut rx = state.subscribe();
    let snapshot = state.snapshot().await;

    if send_event(&mut socket, &PodEvent::Snapshot { pods: snapshot }).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Client fell behind; resync it with a fresh full snapshot.
                        let snapshot = state.snapshot().await;
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
