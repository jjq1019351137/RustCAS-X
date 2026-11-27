CREATE TABLE IF NOT EXISTS invalid_share_links (
    shareurl TEXT PRIMARY KEY,
    drive_type TEXT,
    message TEXT,
    hit_count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_invalid_share_links_drive ON invalid_share_links(drive_type);

CREATE TABLE IF NOT EXISTS share_preview_batch_cache (
    shareurl TEXT PRIMARY KEY,
    drive_type TEXT,
    ok INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    payload_json TEXT NOT NULL DEFAULT '{}',
    checked_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    hit_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_share_preview_batch_cache_exp ON share_preview_batch_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_share_preview_batch_cache_drive ON share_preview_batch_cache(drive_type);
