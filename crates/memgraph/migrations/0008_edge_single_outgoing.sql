ALTER TABLE edge_type ADD COLUMN single_outgoing INTEGER NOT NULL DEFAULT 0;

UPDATE edge_type SET single_outgoing = 1 WHERE name = 'belongs_to';
