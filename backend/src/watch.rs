use futures::TryStreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::watcher::{self, Event};
use kube::runtime::WatchStreamExt;
use kube::{Api, Client};

use crate::resources::pod_to_info;
use crate::state::AppState;

/// Watches pods in `state.namespace` forever, keeping `state` in sync and broadcasting
/// changes to any connected WebSocket clients. Recovers automatically from watch errors
/// (expired resource versions, transient apiserver hiccups, etc).
pub async fn run(state: AppState, client: Client) {
    let api: Api<Pod> = Api::namespaced(client, &state.namespace);
    let mut stream = Box::pin(watcher::watcher(api, watcher::Config::default()).default_backoff());

    let mut init_buffer = Vec::new();
    loop {
        match stream.try_next().await {
            Ok(Some(Event::Init)) => init_buffer.clear(),
            Ok(Some(Event::InitApply(pod))) => init_buffer.push(pod_to_info(&pod)),
            Ok(Some(Event::InitDone)) => {
                let pods = std::mem::take(&mut init_buffer);
                tracing::info!(namespace = %state.namespace, count = pods.len(), "initial pod list loaded");
                state.replace_all(pods).await;
            }
            Ok(Some(Event::Apply(pod))) => state.upsert(pod_to_info(&pod)).await,
            Ok(Some(Event::Delete(pod))) => {
                let name = pod.metadata.name.unwrap_or_default();
                state.remove(&name).await;
            }
            // `watcher()` is an infinite stream (backed by `stream::unfold`), so this never happens.
            Ok(None) => {
                tracing::error!("pod watch stream ended unexpectedly");
                break;
            }
            Err(err) => {
                tracing::warn!(error = %err, "pod watch error, will retry");
            }
        }
    }
}
