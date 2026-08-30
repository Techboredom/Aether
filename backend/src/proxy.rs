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

use crate::auth::CurrentUser;
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
    let mut builder = http::Request::builder().method(req.method().clone()).uri(path_and_query);
    for (name, value) in req.headers().iter() {
        if !is_hop_by_hop(name, is_upgrade) {
            builder = builder.header(name, value);
        }
    }
    if let (Some(env_key), Some(secret_value)) = (&target.env_key, &target.secret_value)
        && let Some(header_value) = credential_header(env_key, secret_value) {
            builder = builder.header(http::header::AUTHORIZATION, header_value);
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
        let (parts, _) = target_response.into_parts();
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
    Ok(response)
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
