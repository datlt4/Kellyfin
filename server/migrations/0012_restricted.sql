-- Per-movie restriction for public sharing: anonymous visitors see only
-- unrestricted items; restricted ones need a signed-in user.
ALTER TABLE items ADD COLUMN restricted INTEGER NOT NULL DEFAULT 0;
