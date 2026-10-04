-- Initial agentic-coding vocabulary. Deliberately small: a kind or edge type
-- earns its row only when it has a query behind it. Descriptions are the
-- agent-facing discovery surface (list_node_kinds / list_edge_types), so they
-- state purpose *and* intended endpoints; the endpoint notes are also the raw
-- material for a future edge_rule table that makes them enforceable.
--
-- INSERT OR IGNORE so an existing dev database that already created some of
-- these names through the API does not fail the migration.

INSERT OR IGNORE INTO node_kind (name, description) VALUES
  ('repo',     'A repository. name is owner/name (e.g. khorshuheng/memgraph); content holds remote URL, local checkout paths and default branch.'),
  ('file',     'A file in a repository. name is the repo-relative path with forward slashes, no leading ./ (e.g. crates/memgraph/src/repository/graph.rs).'),
  ('goal',     'The outcome a user asked for. Root of the work tree; cited by tasks that advance it.'),
  ('task',     'A unit of work with a done-condition. Nest tasks with part_of to form the plan.'),
  ('decision', 'A choice plus its rationale and the alternatives rejected. What future readers must not re-litigate.'),
  ('change',   'A set of landed modifications (commit, patch or PR). Records what actually changed, as opposed to what was planned.'),
  ('test',     'A test intent or case, not a result. A pass/fail line for a specific execution belongs in content.'),
  ('bug',      'A defect or regression, including how it was observed.'),
  ('db_table', 'A database table. name is schema-qualified and lowercase (e.g. public.orders). Foreign keys belong in content as JSON; references is their traversable projection.'),
  ('db_view',  'A database view. name is schema-qualified and lowercase. Records what the view is for and the tables it depends on.');

INSERT OR IGNORE INTO edge_type (name, description) VALUES
  ('contains',    'repo -> file. Physical containment. The only containment hierarchy: walk it up from a file to find its repository.'),
  ('defines',     'file -> db_table | db_view. The migration or DDL that creates the object. Answers "where is this table declared".'),
  ('references',  'db_table -> db_table. A declared foreign key, child pointing at parent. Column pairs and on-delete behaviour stay in the child table content.'),
  ('derived_from','db_view -> db_table | db_view. View lineage. Distinct from references: a dependency, not a constraint, and it breaks queries rather than constraints.'),
  ('reads',       'file -> db_table. The file queries the table.'),
  ('writes',      'file -> db_table. The file inserts, updates or deletes in the table.'),
  ('part_of',     'task -> task | goal. Decomposition. A task tree is the plan; do not introduce a separate plan node.'),
  ('depends_on',  'task -> task. Ordering: the destination must complete first.'),
  ('affects',     'task -> file | db_table. Predicted blast radius, written at planning time.'),
  ('implements',  'change -> task. The change delivers the task.'),
  ('modifies',    'change -> file | db_table. Actual touched set, written after the fact. Keeping this separate from affects is what makes plan-versus-reality measurable.'),
  ('justified_by','change -> decision. The reasoning the change answers to.'),
  ('tested_by',   'file -> test. The test exercises this file.'),
  ('verifies',    'test -> goal | task. The test is evidence that this obligation holds.'),
  ('reveals',     'test -> bug. The test surfaced the defect.'),
  ('fixes',       'change -> bug. The change resolves the defect.');
