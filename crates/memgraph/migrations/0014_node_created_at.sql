ALTER TABLE node ADD COLUMN created_at TEXT NOT NULL DEFAULT '';

UPDATE node SET created_at = updated_at;

