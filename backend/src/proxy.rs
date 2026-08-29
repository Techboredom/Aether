//! Reverse-proxies `/proxy/{deployment_name}/...` straight into the launched
//! pod, injecting its auto-generated credential so there's no login prompt —
//! the "JupyterHub-style" part of the auto-generated-credentials feature.
//!
//! Reaches the pod via `Api<Pod>::portforward` (the same mechanism
//! `kubectl port-forward` uses) rather than a Service/ClusterIP: it tunnels
//! through the API server connection Aether already has, so it works
//! whether the backend runs in-cluster or (as in local dev) against a
//! remote cluster over kubeconfig, and needs no Service object at all.
//!
//! Only templates with `proxy_enabled` (currently just JupyterLab — see
//! `backend/migrations/0005_add_proxy_support.sql`) ever reach this code
//! path; everything else still gets its own public LoadBalancer Service.

use axum::body::{to_bytes, Body};
use axum::extract::{Path, Request, State};
use axum::http;
use axum::http::{HeaderName, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;

use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use common::Role;

const MAX_PROXIED_BODY_BYTES: usize = 20 * 1024 * 1024;

struct ProxyTarget {
    owner_username: String,
    env_key: String,
    secret_value: String,
    container_port: i32,
}

pub async fn handler(
    user: CurrentUser,
    // The wildcard capture only needs to exist for the route to match — the
    // *full* original path (including this `/proxy/{name}/` prefix) is what
    // gets forwarded, unmodified, to the target. Jupyter's `--ServerApp.
    // base_url` (and any future proxied app's equivalent) registers its own
    // routes under that same prefix, so it expects to see it, not have it
    // stripped.
    Path((deployment_name, _rest)): Path<(String, String)>,
    State(state): State<AppState>,
    mut req: Request,
) -> Result<Response, ApiError> {
    let target = load_target(&state, &deployment_name).await?;
    if user.role != Role::Admin && user.username != target.owner_username {
        return Err(ApiError::Forbidden("you don't own this deployment".to_string()));
    }

    let Some(pod_name) = state.running_pod_for_deployment(&deployment_name).await else {
        return Err(ApiError::ProxyUnavailable(format!("{deployment_name} has no running pod yet")));
    };

    let port = target.container_port as u16;
    let pods: Api<Pod> = Api::namespaced(state.client.clone(), &state.namespace);
    let mut forwarder = pods
        .portforward(&pod_name, &[port])
        .await
        .map_err(|err| ApiError::ProxyUnavailable(format!("couldn't reach {deployment_name}: {err}")))?;
    let stream = forwarder
        .take_stream(port)
        .ok_or_else(|| ApiError::ProxyUnavailable(format!("no port-forward stream for {deployment_name}")))?;

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

    // Forwarded verbatim, including the `/proxy/{name}/` prefix — see the
    // note on `_rest` above.
    let path_and_query =
        req.uri().path_and_query().map(|pq| pq.as_str().to_string()).unwrap_or_else(|| "/".to_string());
    let mut builder = http::Request::builder().method(req.method().clone()).uri(path_and_query);
    for (name, value) in req.headers().iter() {
        if !is_hop_by_hop(name, is_upgrade) {
            builder = builder.header(name, value);
        }
    }
    if let Some(header_value) = credential_header(&target.env_key, &target.secret_value) {
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

async fn load_target(state: &AppState, deployment_name: &str) -> Result<ProxyTarget, ApiError> {
    let row: Option<(String, String, String, bool, Option<i32>)> = sqlx::query_as(
        "SELECT owner_username, env_key, secret_value, proxy_enabled, container_port \
         FROM deployment_secrets WHERE deployment_name = $1",
    )
    .bind(deployment_name)
    .fetch_optional(&state.pg)
    .await?;

    let not_found = || ApiError::BadRequest(format!("no proxied deployment named {deployment_name}"));
    let (owner_username, env_key, secret_value, proxy_enabled, container_port) = row.ok_or_else(not_found)?;
    if !proxy_enabled {
        return Err(not_found());
    }
    let container_port = container_port
        .ok_or_else(|| ApiError::ProxyUnavailable(format!("{deployment_name} has no container_port on record")))?;
    Ok(ProxyTarget { owner_username, env_key, secret_value, container_port })
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
