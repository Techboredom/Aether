use std::collections::HashMap;
use std::sync::Arc;

use common::{PodEvent, PodInfo};
use kube::Client;
use sqlx::PgPool;
use tokio::sync::{broadcast, RwLock};

/// Serves each proxied deployment from its own origin
/// (`<name>.<base_domain>`) rather than from a path on Aether's own origin.
/// That separation is what stops a proxied app — which runs code Aether
/// doesn't control — from reaching `/api/*` as whoever is browsing it, since
/// the browser then treats it as a different origin and Aether's host-only
/// session cookie never travels there.
///
/// `None` (neither flag set) keeps the legacy path-based `/proxy/<name>/`
/// behavior, which is same-origin and therefore only appropriate for local
/// development.
#[derive(Clone, Debug)]
pub struct ProxyOrigin {
    /// Where the app itself is served, e.g. `https://aether.example.com`.
    /// Used to send an unauthenticated proxy origin back somewhere the
    /// caller's session cookie actually exists.
    pub app_origin: String,
    /// e.g. `proxy.aether.example.com`, so a deployment named `foo`
    /// is served at `foo.proxy.aether.example.com`.
    pub base_domain: String,
}

impl ProxyOrigin {
    /// The deployment a request's `Host` belongs to, if it names one of our
    /// proxy origins. Matches only a single label in front of the base
    /// domain — `a.b.<base>` is not a deployment, which matters because a
    /// wildcard TLS cert covers exactly one label too.
    pub fn deployment_for_host(&self, host: &str) -> Option<String> {
        // Host may carry a port (`foo.example:8443`); the cert and our
        // routing both key off the hostname alone.
        let host = host.split(':').next().unwrap_or(host).trim_end_matches('.');
        let name = host.strip_suffix(&self.base_domain)?.strip_suffix('.')?;
        if name.is_empty() || name.contains('.') {
            return None;
        }
        Some(name.to_string())
    }

    /// The full origin a given deployment is served from.
    pub fn origin_for(&self, deployment: &str) -> String {
        format!("{}://{deployment}.{}", self.scheme(), self.base_domain)
    }

    /// Mirrors the app origin's scheme — the two are always served the same
    /// way, and it decides whether cookies can be marked `Secure`.
    pub fn scheme(&self) -> &str {
        if self.app_origin.starts_with("http://") { "http" } else { "https" }
    }

    pub fn is_https(&self) -> bool {
        self.scheme() == "https"
    }
}

#[derive(Clone)]
pub struct AppState {
    pub namespace: String,
    pub client: Client,
    pub pg: PgPool,
    /// `None` = legacy same-origin `/proxy/<name>/` mode; see [`ProxyOrigin`].
    pub proxy_origin: Option<ProxyOrigin>,
    pods: Arc<RwLock<HashMap<String, PodInfo>>>,
    events: broadcast::Sender<PodEvent>,
}

impl AppState {
    pub fn new(namespace: String, client: Client, pg: PgPool, proxy_origin: Option<ProxyOrigin>) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            namespace,
            client,
            pg,
            proxy_origin,
            pods: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    /// The URL that opens `deployment` — an absolute URL on its own origin
    /// when per-deployment origins are configured, else the legacy
    /// same-origin path.
    pub fn proxy_url(&self, deployment: &str) -> String {
        match &self.proxy_origin {
            Some(origin) => format!("{}/", origin.origin_for(deployment)),
            None => format!("/proxy/{deployment}/"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> ProxyOrigin {
        ProxyOrigin {
            app_origin: "https://aether.example.com".to_string(),
            base_domain: "proxy.aether.example.com".to_string(),
        }
    }

    #[test]
    fn maps_a_single_label_host_to_its_deployment() {
        let o = origin();
        assert_eq!(o.deployment_for_host("foo.proxy.aether.example.com"), Some("foo".to_string()));
        // A port is part of the Host header but not of the name.
        assert_eq!(o.deployment_for_host("foo.proxy.aether.example.com:8443"), Some("foo".to_string()));
        // Trailing dot is a legal absolute FQDN.
        assert_eq!(o.deployment_for_host("foo.proxy.aether.example.com."), Some("foo".to_string()));
    }

    #[test]
    fn rejects_hosts_that_merely_end_with_the_base_domain() {
        let o = origin();
        // The attacker-registered lookalike: suffix matches, but it is a
        // different domain entirely.
        assert_eq!(o.deployment_for_host("evilproxy.aether.example.com"), None);
        assert_eq!(o.deployment_for_host("notproxy.aether.example.com"), None);
    }

    #[test]
    fn rejects_multi_label_prefixes() {
        // A wildcard cert covers one label, so anything deeper would be
        // served without a matching cert — and would let one deployment
        // shadow another's name.
        assert_eq!(origin().deployment_for_host("a.b.proxy.aether.example.com"), None);
    }

    #[test]
    fn rejects_the_base_domain_and_app_origin_themselves() {
        let o = origin();
        assert_eq!(o.deployment_for_host("proxy.aether.example.com"), None);
        assert_eq!(o.deployment_for_host("aether.example.com"), None);
        assert_eq!(o.deployment_for_host("unrelated.example.com"), None);
        assert_eq!(o.deployment_for_host(""), None);
    }

    #[test]
    fn builds_per_deployment_origins_and_urls() {
        let o = origin();
        assert_eq!(o.origin_for("foo"), "https://foo.proxy.aether.example.com");
        assert!(o.is_https());

        let plain = ProxyOrigin { app_origin: "http://localhost:3000".to_string(), ..origin() };
        assert_eq!(plain.scheme(), "http");
        assert!(!plain.is_https());
    }
}
