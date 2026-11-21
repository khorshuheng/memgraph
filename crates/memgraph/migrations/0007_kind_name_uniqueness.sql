ALTER TABLE node_kind ADD COLUMN unique_name INTEGER NOT NULL DEFAULT 1;

DROP INDEX idx_node_kind_name;
CREATE INDEX idx_node_kind_name ON node (kind_id, name);

CREATE TRIGGER node_name_unique_insert
BEFORE INSERT ON node
WHEN (SELECT unique_name FROM node_kind WHERE id = NEW.kind_id) = 1
BEGIN
  SELECT RAISE(ABORT, 'node name must be unique for this kind')
  WHERE EXISTS (
    SELECT 1 FROM node
    WHERE kind_id = NEW.kind_id AND name = NEW.name AND id IS NOT NEW.id
  );
END;

CREATE TRIGGER node_name_unique_update
BEFORE UPDATE OF name, kind_id ON node
WHEN (SELECT unique_name FROM node_kind WHERE id = NEW.kind_id) = 1
BEGIN
  SELECT RAISE(ABORT, 'node name must be unique for this kind')
  WHERE EXISTS (
    SELECT 1 FROM node
    WHERE kind_id = NEW.kind_id AND name = NEW.name AND id IS NOT NEW.id
  );
END;
