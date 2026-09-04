-- Three more generic args placeholders, same optional/drop-line-if-unset
-- shape as model/context_length/quantization (migrations 0014/0015):
--   served_model_name - the name exposed via the OpenAI-compatible API,
--     separate from the model path/ID (which is often long/ugly).
--   gpu_memory_utilization - fraction of GPU memory one instance reserves
--     (vLLM defaults this to 0.9 - "take almost all of it" - which matters
--     on a multi-tenant, quota-limited, shared-GPU cluster).
--   dtype - precision (e.g. "float16", "bfloat16", "auto"); free text like
--     quantization, since the accepted set varies by engine/version.
ALTER TABLE templates ADD COLUMN served_model_name TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN gpu_memory_utilization DOUBLE PRECISION;
ALTER TABLE templates ADD COLUMN dtype TEXT NOT NULL DEFAULT '';

-- Each on its own args line, same reasoning as 0015: dropped independently
-- when unset rather than reaching the container as a broken flag.
UPDATE templates
SET args = ARRAY['--model={{model}}', '--served-model-name={{served_model_name}}', '--tensor-parallel-size={{accelerator_count}}', '--max-model-len={{context_length}}', '--quantization={{quantization}}', '--gpu-memory-utilization={{gpu_memory_utilization}}', '--dtype={{dtype}}']
WHERE name = 'vLLM';

UPDATE templates
SET args = ARRAY['--model-path={{model}}', '--served-model-name={{served_model_name}}', '--host=0.0.0.0', '--port=30000', '--tp-size={{accelerator_count}}', '--context-length={{context_length}}', '--quantization={{quantization}}', '--mem-fraction-static={{gpu_memory_utilization}}', '--dtype={{dtype}}']
WHERE name = 'SGLang';
