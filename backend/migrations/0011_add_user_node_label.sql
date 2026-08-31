-- An admin-set "key=value" node label pins all of a user's future launches
-- to matching nodes via the pod's nodeSelector. NULL means unrestricted
-- (the scheduler's normal placement, subject only to the CPU-node
-- restriction already baked into Aether's own deployment).
ALTER TABLE users ADD COLUMN IF NOT EXISTS node_label TEXT;
