CREATE TABLE IF NOT EXISTS images (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    image TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Example rows (adjust or remove as needed):
-- INSERT INTO images (name, image, description) VALUES
--   ('Ollama (ROCm)', 'ollama/ollama:rocm', 'Ollama with AMD ROCm GPU support'),
--   ('nginx', 'nginx:stable', 'Stock nginx web server');
