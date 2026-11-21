ALTER TABLE edge_type ADD COLUMN acyclic INTEGER NOT NULL DEFAULT 0;

CREATE TRIGGER edge_contract_insert
BEFORE INSERT ON edge
BEGIN
  SELECT RAISE(ABORT, 'edge type does not accept this source kind')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    JOIN node n ON n.id = NEW.source
    JOIN node_kind k ON k.id = n.kind_id
    WHERE t.id = NEW.edge_type_id
      AND coalesce(t.source_kinds, '') <> ''
      AND instr(',' || t.source_kinds || ',', ',' || k.name || ',') = 0
  );

  SELECT RAISE(ABORT, 'edge type does not accept this destination kind')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    JOIN node n ON n.id = NEW.destination
    JOIN node_kind k ON k.id = n.kind_id
    WHERE t.id = NEW.edge_type_id
      AND coalesce(t.destination_kinds, '') <> ''
      AND instr(',' || t.destination_kinds || ',', ',' || k.name || ',') = 0
  );

  SELECT RAISE(ABORT, 'edge type allows one outgoing edge')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    JOIN edge e ON e.edge_type_id = t.id
    WHERE t.id = NEW.edge_type_id
      AND t.single_outgoing = 1
      AND e.source = NEW.source
      AND e.destination <> NEW.destination
  );

  SELECT RAISE(ABORT, 'edge would create a cycle')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    WHERE t.id = NEW.edge_type_id
      AND t.acyclic = 1
      AND EXISTS (
        WITH RECURSIVE reach(id) AS (
          SELECT NEW.destination
          UNION
          SELECT e.destination
          FROM edge e
          JOIN reach r ON e.source = r.id
          WHERE e.edge_type_id = NEW.edge_type_id
        )
        SELECT 1 FROM reach WHERE id = NEW.source
      )
  );
END;

CREATE TRIGGER edge_contract_update
BEFORE UPDATE ON edge
BEGIN
  SELECT RAISE(ABORT, 'edge type does not accept this source kind')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    JOIN node n ON n.id = NEW.source
    JOIN node_kind k ON k.id = n.kind_id
    WHERE t.id = NEW.edge_type_id
      AND coalesce(t.source_kinds, '') <> ''
      AND instr(',' || t.source_kinds || ',', ',' || k.name || ',') = 0
  );

  SELECT RAISE(ABORT, 'edge type does not accept this destination kind')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    JOIN node n ON n.id = NEW.destination
    JOIN node_kind k ON k.id = n.kind_id
    WHERE t.id = NEW.edge_type_id
      AND coalesce(t.destination_kinds, '') <> ''
      AND instr(',' || t.destination_kinds || ',', ',' || k.name || ',') = 0
  );

  SELECT RAISE(ABORT, 'edge type allows one outgoing edge')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    JOIN edge e ON e.edge_type_id = t.id
    WHERE t.id = NEW.edge_type_id
      AND t.single_outgoing = 1
      AND e.source = NEW.source
      AND e.destination <> NEW.destination
  );

  SELECT RAISE(ABORT, 'edge would create a cycle')
  WHERE EXISTS (
    SELECT 1
    FROM edge_type t
    WHERE t.id = NEW.edge_type_id
      AND t.acyclic = 1
      AND EXISTS (
        WITH RECURSIVE reach(id) AS (
          SELECT NEW.destination
          UNION
          SELECT e.destination
          FROM edge e
          JOIN reach r ON e.source = r.id
          WHERE e.edge_type_id = NEW.edge_type_id
        )
        SELECT 1 FROM reach WHERE id = NEW.source
      )
  );
END;
