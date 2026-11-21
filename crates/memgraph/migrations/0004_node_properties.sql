ALTER TABLE node ADD COLUMN properties TEXT NOT NULL DEFAULT '{}'
  CHECK (json_valid(properties) AND json_type(properties) = 'object');
