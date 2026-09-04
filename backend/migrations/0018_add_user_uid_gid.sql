-- An admin-set UID/GID pair makes every Deployment a user launches run its
-- container as that UID/GID (pod securityContext: runAsUser/runAsGroup,
-- plus fsGroup so volume ownership matches too) instead of the image's own
-- default — the point being different users' pods can each own their own
-- files on a shared NFS mount rather than all colliding on one shared
-- identity. NULL (either or both) means the image's own default, same as
-- node_label's NULL-means-unrestricted convention.
ALTER TABLE users ADD COLUMN IF NOT EXISTS uid INTEGER;
ALTER TABLE users ADD COLUMN IF NOT EXISTS gid INTEGER;
