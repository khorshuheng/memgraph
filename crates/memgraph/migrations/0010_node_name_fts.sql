DROP TRIGGER node_after_insert;
DROP TRIGGER node_after_delete;
DROP TRIGGER node_after_update;
DROP TABLE node_fts;

CREATE VIRTUAL TABLE node_fts USING fts5(
  name,
  description,
  content,
  content = 'node',
  content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO node_fts(node_fts) VALUES ('rebuild');

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
