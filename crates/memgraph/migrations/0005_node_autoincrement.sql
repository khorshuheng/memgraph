CREATE TABLE edge_backup (
  source INTEGER NOT NULL,
  destination INTEGER NOT NULL,
  edge_type_id INTEGER NOT NULL
);

INSERT INTO edge_backup (source, destination, edge_type_id)
  SELECT source, destination, edge_type_id FROM edge;

DROP TABLE edge;

CREATE TABLE node_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind_id INTEGER NOT NULL REFERENCES node_kind(id),
  name TEXT NOT NULL,
  description TEXT NOT NULL,
  content TEXT NOT NULL
);

INSERT INTO node_new (id, kind_id, name, description, content)
  SELECT id, kind_id, name, description, content FROM node;

DROP TABLE node;
ALTER TABLE node_new RENAME TO node;

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

INSERT INTO node_fts(node_fts) VALUES('rebuild');

CREATE TABLE edge (
  source INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  destination INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  edge_type_id INTEGER NOT NULL REFERENCES edge_type(id),
  PRIMARY KEY (source, edge_type_id, destination)
);

INSERT INTO edge (source, destination, edge_type_id)
  SELECT source, destination, edge_type_id FROM edge_backup;

DROP TABLE edge_backup;

CREATE INDEX idx_edge_destination ON edge (destination, edge_type_id);
