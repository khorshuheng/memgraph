CREATE TABLE node_kind (
  id INTEGER PRIMARY KEY,
  version INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE edge_type (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
  resolves INTEGER NOT NULL DEFAULT 0,
  single_outgoing INTEGER NOT NULL DEFAULT 0,
  source_kinds TEXT,
  destination_kinds TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE node (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind_id INTEGER NOT NULL REFERENCES node_kind(id),
  name TEXT NOT NULL,
  description TEXT NOT NULL,
  content TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE node_tag (
  node_id INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  tag TEXT NOT NULL,
  PRIMARY KEY (node_id, tag)
);

CREATE INDEX idx_node_tag_tag ON node_tag (tag, node_id);

CREATE TABLE edge (
  source INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  destination INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  edge_type_id INTEGER NOT NULL REFERENCES edge_type(id),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (source, edge_type_id, destination)
);

CREATE INDEX idx_edge_destination ON edge (destination, edge_type_id);

CREATE VIRTUAL TABLE node_fts USING fts5(
  description,
  content,
  content = 'node',
  content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER node_after_insert AFTER INSERT ON node BEGIN
  INSERT INTO node_fts(rowid, description, content)
  VALUES (new.id, new.description, new.content);
END;

CREATE TRIGGER node_after_delete AFTER DELETE ON node BEGIN
  INSERT INTO node_fts(node_fts, rowid, description, content)
  VALUES ('delete', old.id, old.description, old.content);
END;

CREATE TRIGGER node_after_update AFTER UPDATE ON node BEGIN
  INSERT INTO node_fts(node_fts, rowid, description, content)
  VALUES ('delete', old.id, old.description, old.content);
  INSERT INTO node_fts(rowid, description, content)
  VALUES (new.id, new.description, new.content);
END;
