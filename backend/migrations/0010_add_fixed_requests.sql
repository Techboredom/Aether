-- Used only when expose_resource_requests is false: instead of leaving a
-- launched container's request unset (which Kubernetes then silently
-- defaults to match its limit - Guaranteed QoS, reserving the full limit),
-- the admin can pin a specific request value applied to every launch/edit.
-- NULL means "no fixed value" - falls back to the previous
-- request-unset-entirely behavior.
ALTER TABLE quota_settings ADD COLUMN IF NOT EXISTS fixed_cpu_request TEXT;
ALTER TABLE quota_settings ADD COLUMN IF NOT EXISTS fixed_memory_request TEXT;
