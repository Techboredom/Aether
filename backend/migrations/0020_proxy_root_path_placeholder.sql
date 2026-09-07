-- Both proxied templates hardcode the URL prefix they're served under:
-- JupyterLab's --ServerApp.base_url and RStudio's www-root-path were both
-- set to "/proxy/{{name}}/" (migrations 0005 and 0006). That was correct
-- when a path under Aether's own origin was the only way to reach a
-- proxied app.
--
-- With per-deployment proxy origins (migration 0012) the app owns a whole
-- origin and is served at its root, so that prefix is wrong — and wrong in
-- a way that looks like the app is broken rather than misconfigured:
--
--   RStudio stamps the prefix onto its own redirects, so opening it sends
--   the browser to "/proxy/<name>/auth-sign-in", the proxy forwards that
--   path verbatim (there is no prefix to strip on a per-deployment
--   origin), and rserver 404s because it only serves the bare path. The
--   user sees RStudio's own "The requested page was not found."
--
--   JupyterLab is the mirror image: base_url makes it register its routes
--   *under* the prefix, so a request for "/" — which is what a browser
--   asks for at the root of a per-deployment origin — matches nothing.
--
-- {{proxy_root_path}} resolves to "/" when per-deployment origins are
-- configured and "/proxy/<name>/" when they aren't, so one template is
-- correct under both. See `proxy_root_path` in backend/src/deployments.rs.
--
-- Each UPDATE is scoped to the known-bad forms so a genuinely hand-tailored
-- template is left alone. For RStudio that includes the hardcoded "/" that
-- was the only available hotfix on a backend predating the placeholder (it
-- works, but silently breaks again if per-deployment origins are ever
-- turned off).
UPDATE templates SET
    args = ARRAY['start-notebook.sh', '--ServerApp.base_url={{proxy_root_path}}']
WHERE name = 'JupyterLab'
  AND args IN (
        ARRAY['start-notebook.sh', '--ServerApp.base_url=/proxy/{{name}}/'],
        ARRAY['start-notebook.sh', '--ServerApp.base_url=/']
    );

UPDATE templates SET
    args = ARRAY[
        '/bin/bash', '-c',
        'echo "www-root-path={{proxy_root_path}}" >> /etc/rstudio/disable_auth_rserver.conf && exec /init'
    ]
WHERE name = 'RStudio'
  AND args IN (
        ARRAY[
            '/bin/bash', '-c',
            'echo "www-root-path=/proxy/{{name}}/" >> /etc/rstudio/disable_auth_rserver.conf && exec /init'
        ],
        ARRAY[
            '/bin/bash', '-c',
            'echo "www-root-path=/" >> /etc/rstudio/disable_auth_rserver.conf && exec /init'
        ]
    );
