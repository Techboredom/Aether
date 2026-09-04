-- A first-class "model" field, substituted for the literal string
-- "{{model}}" in a template's args the same way "{{name}}" already is —
-- pulls the one flag every LLM-serving template actually needs edited
-- (vLLM's --model, SGLang's --model-path) out of the free-text args blob
-- into its own field. Works identically whether the value is a Hugging
-- Face model ID or a local filesystem path under a mounted volume (below)
-- — it's just a string substitution either way.
ALTER TABLE templates ADD COLUMN model TEXT NOT NULL DEFAULT '';

-- Mounts an existing PersistentVolumeClaim (provisioned out-of-band, same
-- as the ollama-models PVC already is — Aether never creates one itself)
-- into the launched container. Both columns are set together or not at
-- all; volume_sub_path is optional even when the other two are set.
ALTER TABLE templates ADD COLUMN volume_claim_name TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN volume_mount_path TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN volume_sub_path TEXT NOT NULL DEFAULT '';

-- Move vLLM/SGLang off hand-edited args onto the new model field and the
-- new "{{accelerator_count}}" args placeholder (tensor parallelism should
-- just match however many GPUs were actually requested, not be a second
-- number someone has to keep in sync by hand).
UPDATE templates
SET model = '<huggingface-model-id-or-local-path>',
    args = ARRAY['--model={{model}}', '--tensor-parallel-size={{accelerator_count}}']
WHERE name = 'vLLM';

UPDATE templates
SET model = '<huggingface-model-id-or-local-path>',
    args = ARRAY['--model-path={{model}}', '--host=0.0.0.0', '--port=30000', '--tp-size={{accelerator_count}}']
WHERE name = 'SGLang';
