-- Append-only history of session starts (logins) — separate from
-- `sessions` itself, which is deleted on logout/invalidation and used only
-- for live auth checks. Kept for support/metrics: "when did this user last
-- log in, from where, with what browser."
CREATE TABLE IF NOT EXISTS session_log (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip_address TEXT,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS session_log_user_id_created_at_idx ON session_log (user_id, created_at DESC);

-- Append-only history of what got launched via the Launch tab — separate
-- from `deployment_secrets`, which is keyed by deployment name and gets
-- overwritten if you re-launch under the same name. Kept for
-- support/metrics: "who launched JupyterLab with what resources, who ran
-- vLLM with what model."
CREATE TABLE IF NOT EXISTS launch_log (
    id SERIAL PRIMARY KEY,
    deployment_name TEXT NOT NULL,
    namespace TEXT NOT NULL,
    owner_username TEXT NOT NULL,
    template_name TEXT,
    image TEXT NOT NULL,
    replicas INTEGER NOT NULL,
    cpu_request TEXT,
    cpu_limit TEXT,
    memory_request TEXT,
    memory_limit TEXT,
    accelerator_type TEXT,
    accelerator_count BIGINT,
    container_port INTEGER,
    -- Same [[key, value], ...] shape as templates.env. Any value matching
    -- the launch's generate_secret_for key is redacted before insert (see
    -- deployments.rs) — this log is visible to any logged-in user viewing
    -- their own launches, not just admins, so it must never carry a real
    -- generated credential.
    env JSONB NOT NULL DEFAULT '[]',
    args TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS launch_log_owner_username_created_at_idx ON launch_log (owner_username, created_at DESC);
