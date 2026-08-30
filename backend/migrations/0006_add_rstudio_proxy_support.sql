-- deployment_secrets now tracks proxy-routing metadata even for apps with no
-- credential at all (RStudio can run with its own auth fully disabled,
-- relying solely on Aether's ownership check) — env_key/secret_value become
-- optional, and a new strip_prefix flag controls how the proxy forwards
-- paths (see backend/src/proxy.rs): JupyterLab's --ServerApp.base_url
-- expects the full "/proxy/<name>/" prefix forwarded as-is, but RStudio's
-- www-root-path is the opposite — it only stamps that prefix onto redirects
-- and cookies sent back to the browser, and still expects requests to
-- arrive at the bare path, so the proxy has to strip it first.
ALTER TABLE deployment_secrets ALTER COLUMN env_key DROP NOT NULL;
ALTER TABLE deployment_secrets ALTER COLUMN secret_value DROP NOT NULL;
ALTER TABLE deployment_secrets ADD COLUMN IF NOT EXISTS strip_prefix BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE templates ADD COLUMN IF NOT EXISTS strip_prefix BOOLEAN NOT NULL DEFAULT false;
-- Whether Launch creates a public LoadBalancer Service (the default, same
-- as before) or a ClusterIP-only one. Must be false for any app with no
-- auth of its own, since Aether's proxy ownership check is then the only
-- thing standing between the pod and anyone who can reach its address.
ALTER TABLE templates ADD COLUMN IF NOT EXISTS public_service BOOLEAN NOT NULL DEFAULT true;

-- RStudio switches from "password shown once, public LoadBalancer" to
-- "no auth at all, ClusterIP-only, Aether's login is the only way in" —
-- DISABLE_AUTH=true turns off rserver's own login, and the command
-- appends a www-root-path line to the config file that DISABLE_AUTH swaps
-- in (rocker's /etc/rstudio/disable_auth_rserver.conf) before handing off
-- to the image's normal s6 init (/init) — this image's ENTRYPOINT is empty,
-- so `args` alone becomes the full command line, no wrapping needed.
UPDATE templates SET
    proxy_enabled = true,
    strip_prefix = true,
    public_service = false,
    secret_env_key = NULL,
    env = '[["DISABLE_AUTH", "true"]]',
    args = ARRAY[
        '/bin/bash', '-c',
        'echo "www-root-path=/proxy/{{name}}/" >> /etc/rstudio/disable_auth_rserver.conf && exec /init'
    ],
    notes = 'Runs with authentication fully disabled — click "Open" on the Pods tab once it''s running to go straight in as the "rstudio" user. Aether''s own login (and ownership check) is the only thing gating access, so this never gets a public IP.'
WHERE name = 'RStudio';
