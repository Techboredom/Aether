use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{PodEvent, PodInfo};
use kube::Client;
use sqlx::PgPool;
use tokio::sync::{broadcast, Mutex, MutexGuard, RwLock};

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

/// How many failed logins from one address, within [`LOGIN_FAILURE_WINDOW`],
/// before further attempts are refused outright.
const MAX_LOGIN_FAILURES: usize = 10;
const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(300);

/// Per-source-address failed-login tracking.
///
/// Beyond slowing password guessing, this caps an unauthenticated CPU drain:
/// verifying a password runs argon2, which is expensive *by design*, so an
/// attacker who doesn't care about guessing correctly could otherwise pin the
/// server's cores with a stream of junk logins.
#[derive(Clone)]
pub struct LoginThrottle {
    failures: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>,
    max_failures: usize,
    window: Duration,
}

impl Default for LoginThrottle {
    fn default() -> Self {
        Self::new(MAX_LOGIN_FAILURES, LOGIN_FAILURE_WINDOW)
    }
}

impl LoginThrottle {
    pub fn new(max_failures: usize, window: Duration) -> Self {
        Self { failures: Arc::new(Mutex::new(HashMap::new())), max_failures, window }
    }

    /// Whether `ip` has failed enough logins recently to be turned away
    /// without checking its password at all.
    pub async fn blocked(&self, ip: IpAddr) -> bool {
        let mut failures = self.failures.lock().await;
        let Some(recent) = failures.get_mut(&ip) else { return false };
        recent.retain(|at| at.elapsed() < self.window);
        if recent.is_empty() {
            failures.remove(&ip);
            return false;
        }
        recent.len() >= self.max_failures
    }

    pub async fn record_failure(&self, ip: IpAddr) {
        let mut failures = self.failures.lock().await;
        // Swept here as well as in `blocked`, so a stream of one-off source
        // addresses can't grow this map without bound.
        failures.retain(|_, recent| {
            recent.retain(|at| at.elapsed() < self.window);
            !recent.is_empty()
        });
        failures.entry(ip).or_default().push(Instant::now());
    }

    /// Clears an address's history, so a few typos followed by the right
    /// password don't leave someone throttled.
    pub async fn clear(&self, ip: IpAddr) {
        self.failures.lock().await.remove(&ip);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub namespace: String,
    pub client: Client,
    pub pg: PgPool,
    /// Public origin this app is served from, when it's been configured.
    /// Its scheme is what decides whether cookies may be marked `Secure`.
    pub app_origin: Option<String>,
    /// `None` = legacy same-origin `/proxy/<name>/` mode; see [`ProxyOrigin`].
    pub proxy_origin: Option<ProxyOrigin>,
    launches: Arc<Mutex<()>>,
    login_throttle: LoginThrottle,
    pods: Arc<RwLock<HashMap<String, PodInfo>>>,
    events: broadcast::Sender<PodEvent>,
}

impl AppState {
    pub fn new(
        namespace: String,
        client: Client,
        pg: PgPool,
        app_origin: Option<String>,
        proxy_origin: Option<ProxyOrigin>,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            namespace,
            client,
            pg,
            app_origin,
            proxy_origin,
            launches: Arc::new(Mutex::new(())),
            login_throttle: LoginThrottle::default(),
            pods: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    /// Whether cookies may carry the `Secure` attribute — true only when
    /// this app is served over HTTPS, since a `Secure` cookie is simply never
    /// sent back over plain HTTP and would lock everyone out.
    pub fn cookies_secure(&self) -> bool {
        self.app_origin.as_deref().map(|origin| origin.starts_with("https://")).unwrap_or(false)
    }

    pub async fn login_blocked(&self, ip: IpAddr) -> bool {
        self.login_throttle.blocked(ip).await
    }

    pub async fn record_login_failure(&self, ip: IpAddr) {
        self.login_throttle.record_failure(ip).await;
    }

    pub async fn clear_login_failures(&self, ip: IpAddr) {
        self.login_throttle.clear(ip).await;
    }

    /// Serializes quota-checked writes (launch, scale, edit).
    ///
    /// Quota is enforced by reading current usage and then writing — two
    /// requests interleaving between those steps would both see the
    /// pre-write total and both be allowed, letting a user step over their
    /// quota just by launching twice at once. Held across the Kubernetes
    /// write so the next caller reads a cluster that already includes it.
    /// Global rather than per-user: these writes are infrequent and
    /// short-lived, and one lock is far easier to reason about than a map of
    /// them.
    pub async fn lock_launches(&self) -> MutexGuard<'_, ()> {
        self.launches.lock().await
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

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 10, last])
    }

    #[tokio::test]
    async fn blocks_only_after_the_limit_is_reached() {
        let throttle = LoginThrottle::new(3, Duration::from_secs(300));
        assert!(!throttle.blocked(ip(1)).await);

        throttle.record_failure(ip(1)).await;
        throttle.record_failure(ip(1)).await;
        assert!(!throttle.blocked(ip(1)).await, "under the limit should still be allowed");

        throttle.record_failure(ip(1)).await;
        assert!(throttle.blocked(ip(1)).await);
    }

    #[tokio::test]
    async fn throttling_is_per_address() {
        let throttle = LoginThrottle::new(2, Duration::from_secs(300));
        throttle.record_failure(ip(1)).await;
        throttle.record_failure(ip(1)).await;
        assert!(throttle.blocked(ip(1)).await);
        // One noisy address must not lock everyone else out.
        assert!(!throttle.blocked(ip(2)).await);
    }

    #[tokio::test]
    async fn a_successful_login_clears_the_history() {
        let throttle = LoginThrottle::new(2, Duration::from_secs(300));
        throttle.record_failure(ip(1)).await;
        throttle.record_failure(ip(1)).await;
        assert!(throttle.blocked(ip(1)).await);

        throttle.clear(ip(1)).await;
        assert!(!throttle.blocked(ip(1)).await, "typos then the right password shouldn't leave you locked out");
    }

    #[tokio::test]
    async fn failures_expire_out_of_the_window() {
        let throttle = LoginThrottle::new(1, Duration::from_millis(20));
        throttle.record_failure(ip(1)).await;
        assert!(throttle.blocked(ip(1)).await);

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!throttle.blocked(ip(1)).await, "the lockout should lift once the window passes");
    }

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
