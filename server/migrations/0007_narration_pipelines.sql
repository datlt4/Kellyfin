-- Narration pipelines (node graphs, see pipeline.rs): presets saved by the admin, and the graph each job runs.
CREATE TABLE narration_presets (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    graph TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
ALTER TABLE narration_jobs ADD COLUMN preset TEXT;
ALTER TABLE narration_jobs ADD COLUMN pipeline TEXT;
