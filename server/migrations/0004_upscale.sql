-- Experimental AI upscaling: a job is "upscale this video with this model" and, once done, an extra version of it.
CREATE TABLE upscale_jobs (
    id INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',  -- queued | running | done | failed
    progress REAL NOT NULL DEFAULT 0,
    error TEXT,
    worker TEXT,
    output TEXT,                            -- file under data/upscaled once done
    width INTEGER,
    height INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (item_id, model)
);
CREATE INDEX upscale_jobs_status ON upscale_jobs (status, id);
