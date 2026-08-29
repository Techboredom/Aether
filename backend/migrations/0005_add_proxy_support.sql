-- Templates can opt into being reverse-proxied through Aether itself
-- (backend/src/proxy.rs) instead of getting their own public LoadBalancer
-- Service. deployment_secrets carries the per-launch copy of that flag plus
-- the container port to portforward to, since a template's settings can
-- change after a deployment already exists.
ALTER TABLE templates ADD COLUMN IF NOT EXISTS proxy_enabled BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE deployment_secrets ADD COLUMN IF NOT EXISTS proxy_enabled BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE deployment_secrets ADD COLUMN IF NOT EXISTS container_port INTEGER;

-- Only JupyterLab ships proxy-enabled for now: it has documented support
-- for running behind a path prefix (--ServerApp.base_url) and a simple
-- token-header auth convention. RStudio's equivalent (www-root-path)
-- hasn't been verified against the rocker/rstudio image, so it keeps its
-- own public LoadBalancer + manual credential paste until that's checked.
-- "{{name}}" is substituted with the deployment's own name at launch time.
-- Kubernetes' pod `args` field replaces the image's default CMD entirely (it
-- doesn't append to it), so the jupyter/base-notebook image's own start
-- script has to be named explicitly here or the container just tries (and
-- fails) to exec the flag itself as a program.
UPDATE templates SET
    proxy_enabled = true,
    args = ARRAY['start-notebook.sh', '--ServerApp.base_url=/proxy/{{name}}/'],
    notes = 'Click "Open" on the Pods tab once it''s running — you''ll land in an already-logged-in session, no token needed.'
WHERE name = 'JupyterLab';
