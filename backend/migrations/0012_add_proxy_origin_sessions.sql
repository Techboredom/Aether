-- Per-deployment proxy origins (<name>.<PROXY_BASE_DOMAIN>) are a different
-- host than the app itself, so Aether's own `aether_session` cookie — which
-- is host-only — is never sent there. These two tables back the handoff that
-- gives a proxy origin its own, deliberately narrower credential.

-- One-time, short-lived tokens minted on the app origin (where the caller's
-- session proves who they are and ownership can be checked) and redeemed
-- once on the deployment's proxy origin. Deleted on redemption, so a token
-- leaked via Referer/history/logs is useless after first use.
CREATE TABLE IF NOT EXISTS proxy_auth_tokens (
    token TEXT PRIMARY KEY,
    deployment_name TEXT NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The session a proxy origin gets after redeeming the token above. Scoped to
-- one deployment on purpose: the cookie carrying it is host-only to that
-- subdomain, and every request re-checks that the row's deployment_name
-- matches the host being served, so this grants access to that single
-- deployment and nothing else in Aether.
CREATE TABLE IF NOT EXISTS proxy_sessions (
    token TEXT PRIMARY KEY,
    deployment_name TEXT NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS proxy_sessions_deployment_idx ON proxy_sessions (deployment_name);
