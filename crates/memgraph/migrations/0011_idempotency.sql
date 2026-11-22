CREATE TABLE idempotency_key (
  key TEXT PRIMARY KEY,
  method TEXT NOT NULL,
  path TEXT NOT NULL,
  request_body BLOB NOT NULL,
  status INTEGER,
  response_body BLOB,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_idempotency_key_created ON idempotency_key (created_at);
