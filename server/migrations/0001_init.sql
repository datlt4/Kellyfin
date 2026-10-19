CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT,            -- NULL: imported user, cannot log in until a password is set
    is_admin INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE TABLE sessions (
    token TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE TABLE libraries (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    sort_name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'movies',
    image_ver INTEGER,
    color TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE library_paths (
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    PRIMARY KEY (library_id, path)
);

CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    title TEXT NOT NULL,
    title_locked INTEGER NOT NULL DEFAULT 0,  -- 1: set by user/import, scans keep it
    sort_title TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL,
    duration REAL,
    container TEXT,
    width INTEGER,
    height INTEGER,
    video_codec TEXT,
    bitrate INTEGER,
    streams TEXT,                             -- JSON array from ffprobe
    probed_at INTEGER,
    image_ver INTEGER,
    image_locked INTEGER NOT NULL DEFAULT 0,  -- 1: custom image, never replaced by a frame grab
    color TEXT,
    UNIQUE (library_id, path)
);
CREATE INDEX items_lib_sort ON items (library_id, sort_title, id);
CREATE INDEX items_lib_added ON items (library_id, added_at DESC);
CREATE INDEX items_path ON items (path);

CREATE TABLE user_data (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    position REAL NOT NULL DEFAULT 0,  -- seconds
    played INTEGER NOT NULL DEFAULT 0,
    favorite INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    PRIMARY KEY (user_id, item_id)
);
CREATE INDEX user_data_recent ON user_data (user_id, last_played DESC);
