CREATE TABLE IF NOT EXISTS resource_search_sources (
    key TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 0,
    config_json TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO resource_search_sources (key, enabled, config_json)
VALUES
    ('pansou', 0, '{}'),
    ('cloudsaver', 0, '{}'),
    ('net', 0, '{}');

CREATE TABLE IF NOT EXISTS sync_execution_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sync_execution_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    action TEXT NOT NULL DEFAULT 'copy',
    status TEXT NOT NULL DEFAULT 'ok',
    size INTEGER,
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (sync_execution_id) REFERENCES sync_executions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sync_execution_files_exec
    ON sync_execution_files(sync_execution_id);
