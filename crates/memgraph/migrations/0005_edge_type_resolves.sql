ALTER TABLE edge_type ADD COLUMN resolves INTEGER NOT NULL DEFAULT 0;

UPDATE edge_type SET resolves = 1 WHERE name IN ('implements', 'fixes');
