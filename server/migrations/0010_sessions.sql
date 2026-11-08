-- Session expiry (security review): 0 = legacy row without an expiry (grandfathered).
ALTER TABLE sessions ADD COLUMN expires_at INTEGER NOT NULL DEFAULT 0;
