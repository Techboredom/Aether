-- Admin API tokens: an alternate credential to a session cookie, meant for
-- scripts/automation rather than a browser. Only the SHA-256 hash of the
-- raw value is ever stored (unlike sessions.token, which stores the raw
-- value directly) — these are explicitly meant to be long-lived and handed
-- to systems outside a browser, so a database dump alone should never be
-- replayable as a working credential the way it would be for a token
-- stored in plaintext.
CREATE TABLE IF NOT EXISTS api_tokens (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
