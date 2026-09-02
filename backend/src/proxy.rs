//! Reverse-proxies `/proxy/{deployment_name}/...` straight into the launched
//! pod, injecting its auto-generated credential (if any) so there's no login
//! prompt — the "JupyterHub-style" part of the auto-generated-credentials
//! feature. Some proxied apps (RStudio run with `DISABLE_AUTH=true`) have no
//! credential at all; for those, Aether's own ownership check below is the
//! *only* gate, so their template must also set `public_service = false`
//! (see `deployments.rs`) so nothing can reach them directly.
//!
//! Reaches the pod via its `Service`'s in-cluster `ClusterIP` (whether that
//! Service is itself a public `LoadBalancer` or a `ClusterIP`-only one makes
//! no difference here — both have a `ClusterIP`). This is the conventional
//! in-cluster design and assumes Aether itself runs in-cluster in
//! production; it does **not** work with the backend running locally
//! against a remote cluster, since a `ClusterIP` isn't routable from outside
//! the cluster network — unlike the rest of this app, this one code path
//! can't be exercised from a local dev machine without actually deploying
//! Aether into the cluster.
//!
//! Only templates with `proxy_enabled` (JupyterLab and RStudio — see
//! `backend/migrations/0005_add_proxy_support.sql` and
//! `0006_add_rstudio_proxy_support.sql`) ever reach this code path.
//! `strip_prefix` controls how each one's path gets forwarded — see the
//! note in the handler below.

use axum::body::{to_bytes, Body};
use axum::extract::{Path, Request, State};
use axum::http;
use axum::http::{HeaderName, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use k8s_openapi::api::core::v1::Service;
use kube::api::Api;
use tokio::net::TcpStream;

use crate::auth::{CurrentUser, SESSION_COOKIE};
use crate::error::ApiError;
use crate::state::AppState;
use common::Role;

const MAX_PROXIED_BODY_BYTES: usize = 20 * 1024 * 1024;

struct ProxyTarget {
    owner_username: String,
    env_key: Option<String>,
    secret_value: Option<String>,
    container_port: i32,
    strip_prefix: bool,
}

/// Handles `/proxy/{deployment_name}/{*rest}` — anything with at least one
/// path segment after the deployment name.
pub async fn handler(
    user: CurrentUser,
    Path((deployment_name, rest)): Path<(String, String)>,
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, ApiError> {
    proxy_request(user, deployment_name, rest, state, req).await
}

/// Handles the bare `/proxy/{deployment_name}` and `/proxy/{deployment_name}/`
/// — matchit's wildcard capture in the route above requires at least one
/// character after that slash, so without this, the *exact* URL every
/// "Open" link points to (no trailing segment) would silently fall through
/// to the frontend's own SPA fallback route instead of ever reaching this
/// module. Equivalent to the wildcard route with an empty `rest`.
pub async fn handler_root(
    user: CurrentUser,
    Path(deployment_name): Path<String>,
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, ApiError> {
    proxy_request(user, deployment_name, String::new(), state, req).await
}

async fn proxy_request(
    user: CurrentUser,
    deployment_name: String,
    // Only used when `target.strip_prefix` is set (RStudio's www-root-path
    // model) — see the path-construction note below for why JupyterLab's
    // base_url model needs the full path instead.
    rest: String,
    state: AppState,
    mut req: Request,
) -> Result<Response, ApiError> {
    let target = load_target(&state, &deployment_name).await?;
    if user.role != Role::Admin && user.username != target.owner_username {
        return Err(ApiError::Forbidden("you don't own this deployment".to_string()));
    }

    let port = target.container_port as u16;
    let services: Api<Service> = Api::namespaced(state.client.clone(), &state.namespace);
    let service = services
        .get(&deployment_name)
        .await
        .map_err(|err| ApiError::ProxyUnavailable(format!("couldn't look up {deployment_name}'s Service: {err}")))?;
    let cluster_ip = service
        .spec
        .and_then(|spec| spec.cluster_ip)
        .filter(|ip| ip != "None")
        .ok_or_else(|| ApiError::ProxyUnavailable(format!("{deployment_name} has no ClusterIP yet")))?;

    let stream = tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect((cluster_ip.as_str(), port)))
        .await
        .map_err(|_| ApiError::ProxyUnavailable(format!("timed out connecting to {deployment_name}")))?
        .map_err(|err| ApiError::ProxyUnavailable(format!("couldn't reach {deployment_name}: {err}")))?;

    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(TokioIo::new(stream))
        .await
        .map_err(|err| ApiError::ProxyUnavailable(format!("couldn't connect to {deployment_name}: {err}")))?;
    let conn_deployment_name = deployment_name.clone();
    tokio::spawn(async move {
        if let Err(err) = conn.with_upgrades().await {
            tracing::warn!(deployment = %conn_deployment_name, %err, "proxy connection to pod ended with error");
        }
    });

    let is_upgrade = req.headers().get(http::header::UPGRADE).is_some();
    let client_on_upgrade = is_upgrade.then(|| hyper::upgrade::on(&mut req));

    // Two different apps, two different expectations: JupyterLab's
    // `base_url` wants the full "/proxy/{name}/..." path forwarded as-is (it
    // registers its own routes under that prefix); RStudio's
    // `www-root-path` is the opposite — it only stamps that prefix onto
    // redirects/cookies sent *back* to the browser, and still expects
    // requests to arrive at the bare path, so it has to be stripped first.
    let path_and_query = if target.strip_prefix {
        match req.uri().query() {
            Some(query) => format!("/{rest}?{query}"),
            None => format!("/{rest}"),
        }
    } else {
        req.uri().path_and_query().map(|pq| pq.as_str().to_string()).unwrap_or_else(|| "/".to_string())
    };
    let credential = target
        .env_key
        .as_deref()
        .zip(target.secret_value.as_deref())
        .and_then(|(env_key, secret_value)| credential_header(env_key, secret_value));

    let mut builder = http::Request::builder().method(req.method().clone()).uri(path_and_query);
    if let Some(headers) = builder.headers_mut() {
        *headers = forwarded_headers(req.headers(), is_upgrade, credential.as_deref());
    }

    let body = if is_upgrade {
        Full::new(Bytes::new())
    } else {
        let bytes = to_bytes(req.into_body(), MAX_PROXIED_BODY_BYTES)
            .await
            .map_err(|_| ApiError::BadRequest("request body too large".to_string()))?;
        Full::new(bytes)
    };
    let outbound = builder
        .body(body)
        .map_err(|err| ApiError::ProxyUnavailable(format!("couldn't build proxied request: {err}")))?;

    let mut target_response = sender
        .send_request(outbound)
        .await
        .map_err(|err| ApiError::ProxyUnavailable(format!("proxied request to {deployment_name} failed: {err}")))?;

    if is_upgrade && target_response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let target_upgraded = hyper::upgrade::on(&mut target_response)
            .await
            .map_err(|err| ApiError::ProxyUnavailable(format!("upstream upgrade failed: {err}")))?;
        // `is_upgrade` guarantees this was set above.
        let client_on_upgrade = client_on_upgrade.expect("client_on_upgrade set when is_upgrade");
        tokio::spawn(async move {
            match client_on_upgrade.await {
                Ok(client_upgraded) => {
                    let mut client_io = TokioIo::new(client_upgraded);
                    let mut target_io = TokioIo::new(target_upgraded);
                    if let Err(err) = tokio::io::copy_bidirectional(&mut client_io, &mut target_io).await {
                        tracing::warn!(%err, "proxied websocket tunnel ended with error");
                    }
                }
                Err(err) => tracing::warn!(%err, "client-side upgrade handshake failed"),
            }
        });
        let (mut parts, _) = target_response.into_parts();
        drop_session_set_cookie(&mut parts.headers);
        return Ok(Response::from_parts(parts, Body::empty()));
    }

    let (parts, incoming) = target_response.into_parts();
    let mut response = Response::new(Body::new(incoming));
    *response.status_mut() = parts.status;
    for (name, value) in parts.headers.iter() {
        if !is_hop_by_hop(name, false) {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }
    drop_session_set_cookie(response.headers_mut());
    Ok(response)
}

/// The header set forwarded to the pod: everything the client sent, minus
/// hop-by-hop headers, minus the two that would leak the caller's Aether
/// identity (their session cookie and their own `Authorization`), plus
/// whatever credential Aether manages for this deployment.
fn forwarded_headers(inbound: &http::HeaderMap, is_upgrade: bool, credential: Option<&str>) -> http::HeaderMap {
    let mut out = http::HeaderMap::new();
    for (name, value) in inbound.iter() {
        if is_hop_by_hop(name, is_upgrade) {
            continue;
        }
        // The caller's own Authorization never goes upstream — Aether injects
        // whatever credential it manages for this deployment below, and
        // forwarding the client's too would send the pod two conflicting
        // Authorization headers.
        if name == http::header::AUTHORIZATION {
            continue;
        }
        if name == http::header::COOKIE {
            // Everything except Aether's own session cookie is forwarded:
            // proxied apps set and depend on their own cookies (RStudio's
            // session, JupyterLab's XSRF token), and those come back to us
            // on this same origin.
            if let Ok(cookies) = value.to_str()
                && let Some(forwarded) = strip_session_cookie(cookies)
                && let Ok(forwarded) = forwarded.parse()
            {
                out.append(name.clone(), forwarded);
            }
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    if let Some(credential) = credential
        && let Ok(value) = credential.parse()
    {
        out.insert(http::header::AUTHORIZATION, value);
    }
    out
}

/// Removes Aether's own session cookie from a `Cookie` header on its way to
/// a proxied pod, keeping every other cookie intact. Returns `None` when
/// nothing is left to forward.
///
/// A proxied pod runs code Aether doesn't control — JupyterLab and RStudio
/// run arbitrary user code by design, and `enable_proxy` can be set on any
/// image — so handing it the caller's session cookie would hand it the
/// caller's Aether identity. That matters most for an admin, who can open
/// *anyone's* proxied app: without this, opening a hostile deployment would
/// leak an admin session token to whoever launched it.
fn strip_session_cookie(header: &str) -> Option<String> {
    let kept: Vec<&str> = header
        .split(';')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .filter(|pair| cookie_name(pair) != SESSION_COOKIE)
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

/// Drops any `Set-Cookie` from a proxied pod that would overwrite Aether's
/// own session cookie — otherwise a hostile pod could pin the caller's
/// browser to a session of its choosing (session fixation), since its
/// responses come back on Aether's own origin.
fn drop_session_set_cookie(headers: &mut http::HeaderMap) {
    if !headers.contains_key(http::header::SET_COOKIE) {
        return;
    }
    let kept: Vec<http::HeaderValue> = headers
        .get_all(http::header::SET_COOKIE)
        .iter()
        .filter(|value| value.to_str().map(|v| cookie_name(v) != SESSION_COOKIE).unwrap_or(true))
        .cloned()
        .collect();
    headers.remove(http::header::SET_COOKIE);
    for value in kept {
        headers.append(http::header::SET_COOKIE, value);
    }
}

/// The name part of a `name=value` cookie pair (or of a `Set-Cookie` value,
/// whose attributes trail after the first `;`).
fn cookie_name(pair: &str) -> &str {
    let pair = pair.split(';').next().unwrap_or(pair);
    pair.split('=').next().unwrap_or("").trim()
}

/// (owner_username, env_key, secret_value, proxy_enabled, container_port, strip_prefix)
type ProxyTargetRow = (String, Option<String>, Option<String>, bool, Option<i32>, bool);

async fn load_target(state: &AppState, deployment_name: &str) -> Result<ProxyTarget, ApiError> {
    let row: Option<ProxyTargetRow> = sqlx::query_as(
        "SELECT owner_username, env_key, secret_value, proxy_enabled, container_port, strip_prefix \
         FROM deployment_secrets WHERE deployment_name = $1",
    )
    .bind(deployment_name)
    .fetch_optional(&state.pg)
    .await?;

    let not_found = || ApiError::BadRequest(format!("no proxied deployment named {deployment_name}"));
    let (owner_username, env_key, secret_value, proxy_enabled, container_port, strip_prefix) = row.ok_or_else(not_found)?;
    if !proxy_enabled {
        return Err(not_found());
    }
    let container_port = container_port
        .ok_or_else(|| ApiError::ProxyUnavailable(format!("{deployment_name} has no container_port on record")))?;
    Ok(ProxyTarget { owner_username, env_key, secret_value, container_port, strip_prefix })
}

/// The auth-header convention for each proxied template's generated
/// credential. Adding a new proxy-enabled template later means adding its
/// convention here too, alongside flipping its `proxy_enabled` flag.
fn credential_header(env_key: &str, value: &str) -> Option<String> {
    match env_key {
        "JUPYTER_TOKEN" => Some(format!("token {value}")),
        _ => None,
    }
}

fn is_hop_by_hop(name: &HeaderName, allow_upgrade: bool) -> bool {
    matches!(name.as_str(), "proxy-authenticate" | "proxy-authorization" | "te" | "trailers")
        || (!allow_upgrade && matches!(name.as_str(), "connection" | "keep-alive" | "transfer-encoding" | "upgrade"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a realistic inbound header set: what a browser actually sends
    /// when an admin clicks "Open" on someone else's proxied deployment.
    fn browser_headers(cookie: &str) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::HOST, "aether.example".parse().unwrap());
        headers.insert(http::header::USER_AGENT, "Mozilla/5.0".parse().unwrap());
        headers.insert(http::header::COOKIE, cookie.parse().unwrap());
        headers
    }

    #[test]
    fn session_cookie_never_reaches_the_pod() {
        // The whole point of the fix: a pod runs code Aether doesn't control,
        // so an admin opening a hostile deployment must not hand it their
        // session token.
        let inbound = browser_headers("aether_session=ADMIN_TOKEN; _xsrf=abc");
        let out = forwarded_headers(&inbound, false, None);

        let cookie = out.get(http::header::COOKIE).unwrap().to_str().unwrap();
        assert!(!cookie.contains("ADMIN_TOKEN"), "session token leaked to pod: {cookie}");
        assert!(!cookie.contains("aether_session"));
        assert_eq!(cookie, "_xsrf=abc");
    }

    #[test]
    fn cookie_header_omitted_when_only_the_session_cookie_was_sent() {
        let inbound = browser_headers("aether_session=ADMIN_TOKEN");
        let out = forwarded_headers(&inbound, false, None);
        assert!(!out.contains_key(http::header::COOKIE));
        // Unrelated headers still get through.
        assert_eq!(out.get(http::header::USER_AGENT).unwrap(), "Mozilla/5.0");
    }

    #[test]
    fn callers_authorization_is_replaced_by_aethers_own_credential() {
        let mut inbound = browser_headers("aether_session=ADMIN_TOKEN");
        inbound.insert(http::header::AUTHORIZATION, "Bearer CALLER".parse().unwrap());

        let out = forwarded_headers(&inbound, false, Some("token GENERATED"));

        let auth: Vec<&str> = out.get_all(http::header::AUTHORIZATION).iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(auth, vec!["token GENERATED"], "expected exactly one Authorization header");
    }

    #[test]
    fn drops_hop_by_hop_headers_but_keeps_upgrade_ones_when_upgrading() {
        let mut inbound = browser_headers("_xsrf=abc");
        inbound.insert(http::header::CONNECTION, "Upgrade".parse().unwrap());
        inbound.insert(http::header::UPGRADE, "websocket".parse().unwrap());

        // A normal request: hop-by-hop headers are stripped.
        let plain = forwarded_headers(&inbound, false, None);
        assert!(!plain.contains_key(http::header::CONNECTION));
        assert!(!plain.contains_key(http::header::UPGRADE));

        // A WebSocket upgrade: they must survive or the tunnel never forms
        // (JupyterLab's kernel connection depends on this).
        let upgraded = forwarded_headers(&inbound, true, None);
        assert_eq!(upgraded.get(http::header::UPGRADE).unwrap(), "websocket");
        assert_eq!(upgraded.get(http::header::CONNECTION).unwrap(), "Upgrade");
    }

    #[test]
    fn requests_without_cookies_are_forwarded_unchanged() {
        let mut inbound = http::HeaderMap::new();
        inbound.insert(http::header::ACCEPT, "text/html".parse().unwrap());
        let out = forwarded_headers(&inbound, false, None);
        assert_eq!(out.get(http::header::ACCEPT).unwrap(), "text/html");
        assert!(!out.contains_key(http::header::COOKIE));
    }

    #[test]
    fn strips_only_aethers_own_cookie() {
        // The pod still needs its own cookies (RStudio's session, JupyterLab's
        // XSRF token) — only Aether's session may not cross this boundary.
        assert_eq!(
            strip_session_cookie("csrftoken=abc; aether_session=SECRET; _xsrf=def"),
            Some("csrftoken=abc; _xsrf=def".to_string())
        );
    }

    #[test]
    fn strips_session_cookie_in_any_position() {
        for header in [
            "aether_session=SECRET; keep=1",
            "keep=1; aether_session=SECRET",
            "  aether_session=SECRET  ;  keep=1  ",
        ] {
            assert_eq!(strip_session_cookie(header), Some("keep=1".to_string()), "header: {header}");
        }
    }

    #[test]
    fn drops_header_entirely_when_only_session_cookie_present() {
        assert_eq!(strip_session_cookie("aether_session=SECRET"), None);
        assert_eq!(strip_session_cookie(""), None);
    }

    #[test]
    fn leaves_unrelated_cookies_untouched() {
        assert_eq!(strip_session_cookie("a=1; b=2"), Some("a=1; b=2".to_string()));
    }

    #[test]
    fn does_not_match_on_name_substrings() {
        // Guards against a prefix/contains-style check letting the real
        // cookie through, or eating an unrelated one.
        let header = "not_aether_session=keep; aether_session_x=keep2; aether_session=SECRET";
        assert_eq!(
            strip_session_cookie(header),
            Some("not_aether_session=keep; aether_session_x=keep2".to_string())
        );
    }

    #[test]
    fn handles_values_containing_equals() {
        // Base64-ish values contain '='; splitting must not lose them.
        assert_eq!(
            strip_session_cookie("tok=YWJj==; aether_session=SECRET"),
            Some("tok=YWJj==".to_string())
        );
    }

    #[test]
    fn drops_upstream_attempt_to_overwrite_the_session_cookie() {
        let mut headers = http::HeaderMap::new();
        headers.append(http::header::SET_COOKIE, "aether_session=ATTACKER; Path=/".parse().unwrap());
        headers.append(http::header::SET_COOKIE, "rstudio-session=legit; Path=/".parse().unwrap());

        drop_session_set_cookie(&mut headers);

        let remaining: Vec<&str> =
            headers.get_all(http::header::SET_COOKIE).iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(remaining, vec!["rstudio-session=legit; Path=/"]);
    }

    #[test]
    fn leaves_responses_without_set_cookie_alone() {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, "text/html".parse().unwrap());

        drop_session_set_cookie(&mut headers);

        assert_eq!(headers.len(), 1);
        assert!(!headers.contains_key(http::header::SET_COOKIE));
    }
}
