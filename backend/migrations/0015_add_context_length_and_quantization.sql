-- Two more generic args placeholders, same shape as model/accelerator_count
-- (migration 0014): context_length is a plain positive token count (vLLM's
-- --max-model-len, SGLang's own context flag), quantization a free-text
-- method name (e.g. "awq", "gptq", "fp8") since there's no single fixed set
-- across engines/versions.
ALTER TABLE templates ADD COLUMN context_length BIGINT;
ALTER TABLE templates ADD COLUMN quantization TEXT NOT NULL DEFAULT '';

-- Each on its own args line so it can be dropped independently when unset
-- (see deployments.rs::create_deployment's args-substitution logic) without
-- affecting --model or --tensor-parallel-size on the other lines.
UPDATE templates
SET args = ARRAY['--model={{model}}', '--tensor-parallel-size={{accelerator_count}}', '--max-model-len={{context_length}}', '--quantization={{quantization}}']
WHERE name = 'vLLM';

UPDATE templates
SET args = ARRAY['--model-path={{model}}', '--host=0.0.0.0', '--port=30000', '--tp-size={{accelerator_count}}', '--context-length={{context_length}}', '--quantization={{quantization}}']
WHERE name = 'SGLang';
