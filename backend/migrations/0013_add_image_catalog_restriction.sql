-- Whether non-admin users may launch an image that isn't already known to
-- Aether (a row in the Images catalog, or any Template's own image) — i.e.
-- whether the Launch tab's "Custom" path and free-text image editing are
-- allowed at all for them. Admins are always exempt, same as quota
-- enforcement: this exists to stop a `user` account launching arbitrary
-- images, not to constrain someone who already has unrestricted cluster
-- access via their own kubeconfig regardless of what Aether enforces.
ALTER TABLE quota_settings ADD COLUMN allow_custom_images BOOLEAN NOT NULL DEFAULT true;
