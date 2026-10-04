-- The edge primary key is (source, edge_type_id, destination), so ingoing
-- lookups (`WHERE destination = ?`) cannot use it. Reverse traversal is the
-- common direction for "what points at this?" questions, and
-- `relation_summaries` groups by edge_type_id after filtering on destination.
CREATE INDEX idx_edge_destination ON edge (destination, edge_type_id);
