ALTER TABLE edge_type ADD COLUMN source_kinds TEXT;
ALTER TABLE edge_type ADD COLUMN destination_kinds TEXT;

UPDATE edge_type SET source_kinds = 'repo', destination_kinds = 'file' WHERE name = 'contains';
UPDATE edge_type SET source_kinds = 'file', destination_kinds = 'db_table,db_view' WHERE name = 'defines';
UPDATE edge_type SET source_kinds = 'db_table', destination_kinds = 'db_table' WHERE name = 'references';
UPDATE edge_type SET source_kinds = 'db_view', destination_kinds = 'db_table,db_view' WHERE name = 'derived_from';
UPDATE edge_type SET source_kinds = 'file', destination_kinds = 'db_table' WHERE name = 'reads';
UPDATE edge_type SET source_kinds = 'file', destination_kinds = 'db_table' WHERE name = 'writes';
UPDATE edge_type SET source_kinds = 'task', destination_kinds = 'task,goal' WHERE name = 'part_of';
UPDATE edge_type SET source_kinds = 'task', destination_kinds = 'task' WHERE name = 'depends_on';
UPDATE edge_type SET source_kinds = 'task', destination_kinds = 'file,db_table' WHERE name = 'affects';
UPDATE edge_type SET source_kinds = 'change', destination_kinds = 'task' WHERE name = 'implements';
UPDATE edge_type SET source_kinds = 'change', destination_kinds = 'file,db_table' WHERE name = 'modifies';
UPDATE edge_type SET source_kinds = 'change', destination_kinds = 'decision' WHERE name = 'justified_by';
UPDATE edge_type SET source_kinds = 'file', destination_kinds = 'test' WHERE name = 'tested_by';
UPDATE edge_type SET source_kinds = 'test', destination_kinds = 'goal,task' WHERE name = 'verifies';
UPDATE edge_type SET source_kinds = 'test', destination_kinds = 'bug' WHERE name = 'reveals';
UPDATE edge_type SET source_kinds = 'change', destination_kinds = 'bug' WHERE name = 'fixes';

INSERT OR IGNORE INTO edge_type (name, description, source_kinds, destination_kinds) VALUES
  ('belongs_to', 'goal -> repo. The project a goal advances. Anchors a work tree to its repository, so a repository can be asked for its open tasks.', 'goal', 'repo');
