-- Multi share-link list per drama task (Python shareurl_list_json parity).
ALTER TABLE tasks ADD COLUMN shareurl_list_json TEXT NOT NULL DEFAULT '[]';
