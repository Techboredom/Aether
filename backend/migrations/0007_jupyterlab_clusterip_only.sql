-- JupyterLab now follows the same pattern RStudio already uses: no public
-- LoadBalancer at all, since the only supported way in is already Aether's
-- own proxy (Authorization: token header injected server-side) — a public
-- IP straight to the pod would just be a second, unauthenticated way to
-- reach the same notebook server.
UPDATE templates SET
    public_service = false,
    notes = 'Click "Open" on the Pods tab once it''s running — you''ll land in an already-logged-in session, no token needed. Aether''s proxy is the only way in, so this never gets a public IP.'
WHERE name = 'JupyterLab';
