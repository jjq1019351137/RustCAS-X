CREATE TABLE IF NOT EXISTS drive_accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    drive_type TEXT NOT NULL,
    cookie TEXT NOT NULL DEFAULT '',
    config_json TEXT,
    profile_json TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    is_default INTEGER NOT NULL DEFAULT 0,
    capacity_warning_threshold INTEGER NOT NULL DEFAULT 85,
    runtime_status TEXT,
    last_checked_at TEXT,
    last_error TEXT,
    probe_fail_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_drive_accounts_type ON drive_accounts(drive_type);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS task_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    stage TEXT,
    message TEXT,
    run_log TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sync_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sync_task_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    run_log TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    FOREIGN KEY (sync_task_id) REFERENCES sync_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS deferred_sync_deletes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sync_task_uid TEXT,
    base_path TEXT NOT NULL,
    dst_rel TEXT NOT NULL,
    due_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
