-- Cluster-wide default quota, a single-row table (id is always 1). NULL in
-- any column means "unlimited" for that dimension. Quotas are checked
-- against resource *limits* (not requests) since interactive workloads are
-- bursty and it's peak usage, not steady-state reservation, that risks
-- starving other users of the shared cluster.
CREATE TABLE IF NOT EXISTS quota_settings (
    id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    cpu_limit TEXT,
    memory_limit TEXT,
    gpu_limit INTEGER,
    -- Whether the Launch tab and the Pods tab's manage panel show separate
    -- "request" fields at all. When false, only limits are shown/settable
    -- and no request is ever sent - Kubernetes itself then defaults a
    -- container's request to match its limit when only a limit is given.
    expose_resource_requests BOOLEAN NOT NULL DEFAULT true
);
INSERT INTO quota_settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;

-- Per-user overrides. A user with no row here is bound by quota_settings'
-- global defaults instead. Unlike quota_settings, a row existing at all
-- means "this user has a custom quota" - each of its three fields is
-- independently nullable, NULL meaning unlimited for that one dimension
-- (not "fall back to the global value" - once overridden, it's fully
-- overridden).
CREATE TABLE IF NOT EXISTS user_quotas (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    cpu_limit TEXT,
    memory_limit TEXT,
    gpu_limit INTEGER
);
