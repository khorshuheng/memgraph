use std::sync::Arc;

use crate::model::{EdgeType, NodeKind};
use crate::repository::graph::GraphRepository;

const KINDS: [(&str, &str); 10] = [
    (
        "repo",
        "A repository. name is owner/name; content holds remote URL and local paths.",
    ),
    (
        "file",
        "A file in a repository. name is the repo-relative path.",
    ),
    (
        "goal",
        "The outcome a user asked for. Root of the work tree.",
    ),
    ("task", "A unit of work with a done-condition."),
    (
        "decision",
        "A choice plus its rationale and rejected alternatives.",
    ),
    ("change", "A set of landed modifications."),
    ("test", "A test intent or case, not a result."),
    (
        "bug",
        "A defect or regression, including how it was observed.",
    ),
    (
        "db_table",
        "A database table. name is schema-qualified and lowercase.",
    ),
    (
        "db_view",
        "A database view. name is schema-qualified and lowercase.",
    ),
];

const EDGE_TYPES: [(&str, &str, bool, bool, &str, &str); 17] = [
    ("contains", "repo -> file.", false, false, "repo", "file"),
    (
        "defines",
        "file -> db_table | db_view.",
        false,
        false,
        "file",
        "db_table,db_view",
    ),
    (
        "references",
        "db_table -> db_table.",
        false,
        false,
        "db_table",
        "db_table",
    ),
    (
        "derived_from",
        "db_view -> db_table | db_view.",
        false,
        false,
        "db_view",
        "db_table,db_view",
    ),
    (
        "reads",
        "file -> db_table.",
        false,
        false,
        "file",
        "db_table",
    ),
    (
        "writes",
        "file -> db_table.",
        false,
        false,
        "file",
        "db_table",
    ),
    (
        "part_of",
        "task -> task | goal.",
        false,
        false,
        "task",
        "task,goal",
    ),
    ("depends_on", "task -> task.", false, false, "task", "task"),
    (
        "affects",
        "task -> file | db_table.",
        false,
        false,
        "task",
        "file,db_table",
    ),
    (
        "implements",
        "change -> task.",
        true,
        false,
        "change",
        "task",
    ),
    (
        "modifies",
        "change -> file | db_table.",
        false,
        false,
        "change",
        "file,db_table",
    ),
    (
        "justified_by",
        "change -> decision.",
        false,
        false,
        "change",
        "decision",
    ),
    ("tested_by", "file -> test.", false, false, "file", "test"),
    (
        "verifies",
        "test -> goal | task.",
        false,
        false,
        "test",
        "goal,task",
    ),
    ("reveals", "test -> bug.", false, false, "test", "bug"),
    ("fixes", "change -> bug.", true, false, "change", "bug"),
    ("belongs_to", "goal -> repo.", false, true, "goal", "repo"),
];

pub async fn seed_vocabulary(repository: &Arc<dyn GraphRepository>) {
    for (name, description) in KINDS {
        if let Err(error) = repository
            .create_node_kind(&NodeKind {
                id: 0,
                name: name.to_string(),
                description: description.to_string(),
                updated_at: String::new(),
            })
            .await
        {
            panic!("failed to seed node kind {name}: {error}");
        }
    }
    for (name, description, resolves, single_outgoing, source_kinds, destination_kinds) in
        EDGE_TYPES
    {
        if let Err(error) = repository
            .create_edge_type(&EdgeType {
                id: 0,
                name: name.to_string(),
                description: description.to_string(),
                resolves,
                single_outgoing,
                source_kinds: Some(source_kinds.to_string()),
                destination_kinds: Some(destination_kinds.to_string()),
                updated_at: String::new(),
            })
            .await
        {
            panic!("failed to seed edge type {name}: {error}");
        }
    }
}
