CREATE TABLE IF NOT EXISTS templates (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    image TEXT NOT NULL,
    container_port INTEGER,
    cpu_request TEXT NOT NULL DEFAULT '',
    cpu_limit TEXT NOT NULL DEFAULT '',
    memory_request TEXT NOT NULL DEFAULT '',
    memory_limit TEXT NOT NULL DEFAULT '',
    accelerator_type TEXT NOT NULL DEFAULT '',
    accelerator_count BIGINT,
    -- Array of [key, value] pairs, e.g. [["PASSWORD", ""]]. A blank value means
    -- "scaffold this key but let the launcher fill it in" (see backend/src/deployments.rs,
    -- which drops empty-valued env vars so an image's own default behavior still applies).
    env JSONB NOT NULL DEFAULT '[]',
    args TEXT[] NOT NULL DEFAULT '{}',
    notes TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed with the templates that used to be hardcoded in the frontend, so
-- existing behavior is preserved after moving them into the database.
INSERT INTO templates (name, image, container_port, cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count, env, args, notes) VALUES
(
    'Ollama',
    'ollama/ollama:latest',
    11434,
    '500m', '2', '2Gi', '8Gi',
    'amd.com/gpu', 1,
    '[]',
    '{}',
    'No authentication by default — anyone who can reach the Service can use it. Pull models via the Ollama API/CLI after it starts; there''s no persistent storage yet, so pulled models are lost if the pod restarts.'
),
(
    'vLLM',
    'vllm/vllm-openai:latest',
    8000,
    '1', '4', '4Gi', '16Gi',
    'amd.com/gpu', 1,
    '[]',
    ARRAY['--model=<huggingface-model-id>'],
    'Edit the --model argument below to the Hugging Face model you want to serve. No persistent storage yet, so the model is re-downloaded on every restart.'
),
(
    'SGLang',
    'lmsysorg/sglang:latest',
    30000,
    '1', '4', '4Gi', '16Gi',
    'amd.com/gpu', 1,
    '[]',
    ARRAY['--model-path=<huggingface-model-id>', '--host=0.0.0.0', '--port=30000'],
    'Edit --model-path to the Hugging Face model you want to serve. No persistent storage yet, so the model is re-downloaded on every restart.'
),
(
    'JupyterLab',
    'jupyter/base-notebook:latest',
    8888,
    '250m', '2', '512Mi', '4Gi',
    '', NULL,
    '[["JUPYTER_TOKEN", ""]]',
    '{}',
    'Set JUPYTER_TOKEN to choose your own login token, or leave it blank and read the auto-generated one from the pod''s logs after it starts (open the pod''s detail panel on the Pods tab).'
),
(
    'RStudio',
    'rocker/rstudio:latest',
    8787,
    '250m', '2', '512Mi', '4Gi',
    '', NULL,
    '[["PASSWORD", ""]]',
    '{}',
    'Username is always "rstudio". Set PASSWORD to choose your own login password, or leave it blank and read the auto-generated one from the pod''s logs after it starts.'
);
