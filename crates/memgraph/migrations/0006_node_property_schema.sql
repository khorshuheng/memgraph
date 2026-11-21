CREATE TABLE node_kind_property (
  kind_id INTEGER NOT NULL REFERENCES node_kind(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value_type TEXT NOT NULL DEFAULT 'any'
    CHECK (value_type IN ('any', 'string', 'number', 'integer', 'boolean', 'array', 'object')),
  required INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (kind_id, key)
);
