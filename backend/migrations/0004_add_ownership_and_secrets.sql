-- Optional env var name that should get an auto-generated random value at
-- launch time instead of being shown as an editable field (e.g. JupyterLab's
-- JUPYTER_TOKEN, RStudio's PASSWORD, vLLM's VLLM_API_KEY).
ALTER TABLE templates ADD COLUMN IF NOT EXISTS secret_env_key TEXT;

UPDATE templates SET
    secret_env_key = 'JUPYTER_TOKEN',
    env = '[]',
    notes = 'A login token is generated automatically and shown here (and on the Pods tab) once it''s running — there''s no need to set one yourself.'
WHERE name = 'JupyterLab';

UPDATE templates SET
    secret_env_key = 'PASSWORD',
    env = '[]',
    notes = 'Username is always "rstudio". A login password is generated automatically and shown here (and on the Pods tab) once it''s running.'
WHERE name = 'RStudio';

UPDATE templates SET
    secret_env_key = 'VLLM_API_KEY',
    notes = 'Edit the --model argument below to the Hugging Face model you want to serve. An API key is generated automatically (shown here and on the Pods tab) — pass it as "Authorization: Bearer <key>" when calling the OpenAI-compatible API. No persistent storage yet, so the model is re-downloaded on every restart.'
WHERE name = 'vLLM';

-- One generated secret per launched Deployment, keyed by its (stable) name —
-- pod names change across restarts but the Deployment name the user chose
-- doesn't. Re-launching under the same name replaces the stored value
-- (ON CONFLICT below), matching the Deployment itself being replaced.
CREATE TABLE IF NOT EXISTS deployment_secrets (
    deployment_name TEXT PRIMARY KEY,
    namespace TEXT NOT NULL,
    env_key TEXT NOT NULL,
    secret_value TEXT NOT NULL,
    owner_username TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
