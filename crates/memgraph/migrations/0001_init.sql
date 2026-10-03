CREATE TABLE node (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL,
  content TEXT NOT NULL
);

CREATE TABLE relation (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL
);

CREATE TABLE edge (
  source INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  destination INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  relation_id INTEGER NOT NULL REFERENCES relation(id) ON DELETE CASCADE,
  PRIMARY KEY (source, relation_id, destination)
);

CREATE VIRTUAL TABLE node_fts USING fts5(
  name, description, content,
  content = 'node',
  content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER node_after_insert AFTER INSERT ON node BEGIN
  INSERT INTO node_fts(rowid, name, description, content)
  VALUES (new.id, new.name, new.description, new.content);
END;

CREATE TRIGGER node_after_delete AFTER DELETE ON node BEGIN
  INSERT INTO node_fts(node_fts, rowid, name, description, content)
  VALUES ('delete', old.id, old.name, old.description, old.content);
END;

CREATE TRIGGER node_after_update AFTER UPDATE ON node BEGIN
  INSERT INTO node_fts(node_fts, rowid, name, description, content)
  VALUES ('delete', old.id, old.name, old.description, old.content);
  INSERT INTO node_fts(rowid, name, description, content)
  VALUES (new.id, new.name, new.description, new.content);
END;
