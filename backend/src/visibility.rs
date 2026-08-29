use std::collections::HashMap;

use common::{PodCredential, PodInfo, Role};
use sqlx::PgPool;

use crate::auth::CurrentUser;

/// Filters `pods` down to what `user` is allowed to see — everything, for an
/// admin; only pods they launched themselves, for everyone else — and
/// attaches each visible pod's stored credential (if its template generated one).
pub async fn visible_to(pods: Vec<PodInfo>, user: &CurrentUser, pg: &PgPool) -> Vec<PodInfo> {
    let mut visible: Vec<PodInfo> = if user.role == Role::Admin {
        pods
    } else {
        pods.into_iter().filter(|p| p.owner.as_deref() == Some(user.username.as_str())).collect()
    };

    let deployment_names: Vec<String> = visible.iter().filter_map(|p| p.deployment_name.clone()).collect();
    if deployment_names.is_empty() {
        return visible;
    }

    let secrets = match load_secrets(pg, &deployment_names).await {
        Ok(secrets) => secrets,
        Err(err) => {
            tracing::warn!(error = %err, "failed to load deployment secrets");
            return visible;
        }
    };
    for pod in &mut visible {
        if let Some(name) = &pod.deployment_name
            && let Some(stored) = secrets.get(name) {
                pod.credential = Some(stored.credential.clone());
                pod.proxy_path = stored.proxy_enabled.then(|| format!("/proxy/{name}/"));
            }
    }
    visible
}

/// Whether `user` is allowed to see `pod` at all — used to filter individual
/// live Upsert events on the WebSocket.
pub fn can_see(pod: &PodInfo, user: &CurrentUser) -> bool {
    user.role == Role::Admin || pod.owner.as_deref() == Some(user.username.as_str())
}

/// Attaches `pod`'s stored credential (and proxy path, if proxy-enabled) in
/// place. Used for single live Upsert events, where a full batch lookup
/// would be overkill.
pub async fn attach_credential(pod: &mut PodInfo, pg: &PgPool) {
    let Some(name) = &pod.deployment_name else { return };
    if let Ok(Some((env_key, value, proxy_enabled))) = sqlx::query_as::<_, (String, String, bool)>(
        "SELECT env_key, secret_value, proxy_enabled FROM deployment_secrets WHERE deployment_name = $1",
    )
    .bind(name)
    .fetch_optional(pg)
    .await
    {
        pod.credential = Some(PodCredential { env_key, value });
        pod.proxy_path = proxy_enabled.then(|| format!("/proxy/{name}/"));
    }
}

struct StoredSecret {
    credential: PodCredential,
    proxy_enabled: bool,
}

async fn load_secrets(pg: &PgPool, deployment_names: &[String]) -> Result<HashMap<String, StoredSecret>, sqlx::Error> {
    let rows: Vec<(String, String, String, bool)> = sqlx::query_as(
        "SELECT deployment_name, env_key, secret_value, proxy_enabled FROM deployment_secrets WHERE deployment_name = ANY($1)",
    )
    .bind(deployment_names)
    .fetch_all(pg)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, env_key, value, proxy_enabled)| (name, StoredSecret { credential: PodCredential { env_key, value }, proxy_enabled }))
        .collect())
}
