-- Per-user OpenRouter API keys for vision-model OCR (2.13.0): a user pastes their own key in Settings → Profile,
-- it is AES-256-GCM encrypted with KELLYFIN_OPENROUTER_KEY (an env var, never in the DB), the model pick is free-only
-- and each user picks a primary and a fallback model. Jobs remember who queued them so the worker can fetch that
-- user's key for OpenRouter OCR jobs (legacy jobs have NULL).
CREATE TABLE user_openrouter (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    encrypted_key TEXT NOT NULL DEFAULT '',  -- base64(nonce || aes-256-gcm ciphertext); '' = unset
    primary_model TEXT NOT NULL DEFAULT 'dots-studio/dots-3-note-preview:free',
    fallback_model TEXT NOT NULL DEFAULT 'nvidia/nemotron-3.5-super-vl:free',
    updated_at INTEGER NOT NULL
);
ALTER TABLE narration_jobs ADD COLUMN user_id INTEGER;
