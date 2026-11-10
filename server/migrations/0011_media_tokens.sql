-- Scoped media tokens: <video>/<track>/<img> cannot send headers, so they get
-- their own token that MediaUser accepts and the JSON API never does. Only the
-- hash is stored, so a database copy hands out nothing usable.
CREATE TABLE media_tokens (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
