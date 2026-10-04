CREATE TABLE node_access (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL,
  node_id INTEGER NOT NULL,
  node_name TEXT NOT NULL,
  node_kind TEXT NOT NULL,
  action TEXT NOT NULL,
  accessed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_node_access_session ON node_access (session_id, accessed_at);
CREATE INDEX idx_node_access_node ON node_access (node_id, accessed_at);
