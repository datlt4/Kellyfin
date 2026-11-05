-- Queue control: a job moved to the top gets a higher priority (claimed first); status 'paused' is set in code.
ALTER TABLE narration_jobs ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;
