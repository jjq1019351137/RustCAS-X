-- Persist transferred history on tasks (parity with transferred_history_json).
ALTER TABLE tasks ADD COLUMN transferred_history_json TEXT NOT NULL DEFAULT '[]';
