-- Narration (thuyết minh): subtitles and voiced audio made by a film-sub worker (narrator/), stored under
-- data/narration/<item>/; the video files are never touched.
CREATE TABLE narration_jobs (
    id INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL UNIQUE REFERENCES items(id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'auto',    -- spoken language of the video ("auto" = detect), e.g. "ko"
    langs TEXT NOT NULL,                    -- narration languages, e.g. "vi,zh"
    status TEXT NOT NULL DEFAULT 'queued',  -- queued | running (worker) | muxing (server) | done | failed
    stage INTEGER NOT NULL DEFAULT 0,       -- 1 subtitles, 2 translation, 3 voice (TTS), 4 mux
    step TEXT,                              -- what the worker is doing (shown in Settings)
    progress REAL NOT NULL DEFAULT 0,
    error TEXT,
    worker TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX narration_jobs_status ON narration_jobs (status, id);
