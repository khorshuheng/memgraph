UPDATE node
SET name = name || ' [duplicate #' || id || ']',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id NOT IN (SELECT id FROM (SELECT min(id) AS id FROM node GROUP BY kind_id, name));

CREATE UNIQUE INDEX idx_node_kind_name ON node (kind_id, name);
