-- readiness_path: HTTP path for a readinessProbe against container_port,
-- e.g. "/health". Empty means no probe, same as the args placeholders
-- above — without one, Kubernetes considers a container "Ready" the
-- instant its process starts, which for an LLM server means the Pods tab
-- (and any rolling update) treats it as serving *before* it's actually
-- finished loading the model into GPU memory and can answer requests.
ALTER TABLE templates ADD COLUMN readiness_path TEXT NOT NULL DEFAULT '';

UPDATE templates SET readiness_path = '/health' WHERE name IN ('vLLM', 'SGLang');
UPDATE templates SET readiness_path = '/' WHERE name = 'Ollama';
