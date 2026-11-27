CREATE TABLE IF NOT EXISTS tmdb_media_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_type TEXT NOT NULL,
    tmdb_id INTEGER NOT NULL,
    language TEXT NOT NULL DEFAULT 'zh-CN',
    poster_language TEXT NOT NULL DEFAULT 'zh-CN',
    display_title TEXT,
    original_title TEXT,
    year TEXT,
    status TEXT,
    payload_json TEXT NOT NULL DEFAULT '{}',
    update_weekdays_json TEXT NOT NULL DEFAULT '[]',
    fetched_at TEXT,
    expires_at TEXT,
    last_accessed_at TEXT,
    refresh_in_progress INTEGER NOT NULL DEFAULT 0,
    refresh_started_at TEXT,
    fail_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    UNIQUE(media_type, tmdb_id, language, poster_language)
);

CREATE INDEX IF NOT EXISTS idx_tmdb_media_cache_lookup
    ON tmdb_media_cache(media_type, tmdb_id);
