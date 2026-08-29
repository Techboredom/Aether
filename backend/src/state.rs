use std::collections::HashMap;
use std::sync::Arc;

use common::{PodEvent, PodInfo};
use kube::Client;
use sqlx::PgPool;
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub namespace: String,
    pub client: Client,
    pub pg: PgPool,
    pods: Arc<RwLock<HashMap<String, PodInfo>>>,
    events: broadcast::Sender<PodEvent>,
}

impl AppState {
    pub fn new(namespace: String, client: Client, pg: PgPool) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            namespace,
            client,
            pg,
            pods: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PodEvent> {
        self.events.subscribe()
    }

    pub async fn snapshot(&self) -> Vec<PodInfo> {
        self.pods.read().await.values().cloned().collect()
    }

    /// Inserts or replaces a pod's info and broadcasts the change to any live listeners.
    pub async fn upsert(&self, pod: PodInfo) {
        self.pods.write().await.insert(pod.name.clone(), pod.clone());
        // Ignore send errors: they only mean no WebSocket client is currently connected.
        let _ = self.events.send(PodEvent::Upsert { pod: Box::new(pod) });
    }

    /// Replaces the whole pod set atomically (used after the watcher's initial list completes).
    pub async fn replace_all(&self, pods: Vec<PodInfo>) {
        let mut guard = self.pods.write().await;
        guard.clear();
        for pod in pods {
            guard.insert(pod.name.clone(), pod);
        }
    }

    pub async fn remove(&self, name: &str) {
        self.pods.write().await.remove(name);
        let _ = self.events.send(PodEvent::Delete { name: name.to_string() });
    }
}
