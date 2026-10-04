CREATE TABLE node_kind (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE edge_type (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
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

CREATE TABLE edge (
  source INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  destination INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  edge_type_id INTEGER NOT NULL REFERENCES edge_type(id),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (source, edge_type_id, destination)
);

CREATE VIRTUAL TABLE node_fts USING fts5(
  description,
  content = 'node',
  content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE node_fts_vocab USING fts5vocab(node_fts, 'row');

CREATE TRIGGER node_after_insert AFTER INSERT ON node BEGIN
  INSERT INTO node_fts(rowid, description)
  VALUES (new.id, new.description);
END;

CREATE TRIGGER node_after_delete AFTER DELETE ON node BEGIN
  INSERT INTO node_fts(node_fts, rowid, description)
  VALUES ('delete', old.id, old.description);
END;

CREATE TRIGGER node_after_update AFTER UPDATE ON node BEGIN
  INSERT INTO node_fts(node_fts, rowid, description)
  VALUES ('delete', old.id, old.description);
  INSERT INTO node_fts(rowid, description)
  VALUES (new.id, new.description);
END;
