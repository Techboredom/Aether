use std::collections::HashMap;

use common::{PodEvent, PodInfo};
use futures::StreamExt;
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::prelude::*;

fn ws_url() -> String {
    let location = web_sys::window().expect("window").location();
    let is_https = location.protocol().unwrap_or_default() == "https:";
    let host = location.host().unwrap_or_default();
    let scheme = if is_https { "wss" } else { "ws" };
    format!("{scheme}://{host}/ws")
}

/// Connects to the pods WebSocket and keeps `pods` in sync, reconnecting with a fixed
/// backoff if the connection drops (e.g. the backend restarts).
pub async fn run(
    pods: RwSignal<HashMap<String, PodInfo>>,
    connected: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) {
    loop {
        match WebSocket::open(&ws_url()) {
            Ok(ws) => {
                connected.set(true);
                error.set(None);
                let mut stream = ws;
                while let Some(msg) = stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => match serde_json::from_str::<PodEvent>(&text) {
                            Ok(event) => apply_event(pods, event),
                            Err(err) => error.set(Some(format!("bad event from server: {err}"))),
                        },
                        Ok(Message::Bytes(_)) => {}
                        Err(_) => break,
                    }
                }
            }
            Err(err) => error.set(Some(format!("failed to connect: {err}"))),
        }
        connected.set(false);
        gloo_timers::future::TimeoutFuture::new(2_000).await;
    }
}

fn apply_event(pods: RwSignal<HashMap<String, PodInfo>>, event: PodEvent) {
    match event {
        PodEvent::Snapshot { pods: list } => {
            pods.set(list.into_iter().map(|p| (p.name.clone(), p)).collect());
        }
        PodEvent::Upsert { pod } => {
            pods.update(|map| {
                map.insert(pod.name.clone(), *pod);
            });
        }
        PodEvent::Delete { name } => {
            pods.update(|map| {
                map.remove(&name);
            });
        }
    }
}
