use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};

use crate::model::graph::{Edge, Node, Relation};

const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

const NODE_COLUMNS: &str = "id, kind, name, description, content";

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub node: Node,
    pub score: f64,
}

pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &str) -> Result<Self, sqlx::Error> {
        Self::connect(path, 5).await
    }

    pub async fn in_memory() -> Result<Self, sqlx::Error> {
        Self::connect(":memory:", 1).await
    }

    async fn connect(path: &str, max_connections: u32) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        migrate(&self.pool).await
    }

    pub async fn upsert_node(&self, node: &Node) -> Result<Node, sqlx::Error> {
        if node.id == 0 {
            sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (kind, name, description, content) VALUES (?, ?, ?, ?) \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(&node.kind)
            .bind(&node.name)
            .bind(&node.description)
            .bind(&node.content)
            .fetch_one(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (id, kind, name, description, content) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, \
                 description = excluded.description, content = excluded.content \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(node.id)
            .bind(&node.kind)
            .bind(&node.name)
            .bind(&node.description)
            .bind(&node.content)
            .fetch_one(&self.pool)
            .await
        }
    }

    pub async fn get_node(&self, id: i64) -> Result<Option<Node>, sqlx::Error> {
        sqlx::query_as::<_, Node>(&format!("SELECT {NODE_COLUMNS} FROM node WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn delete_node(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM node WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_nodes(&self) -> Result<Vec<Node>, sqlx::Error> {
        sqlx::query_as::<_, Node>(&format!("SELECT {NODE_COLUMNS} FROM node ORDER BY id"))
            .fetch_all(&self.pool)
            .await
    }

    pub async fn upsert_relation(&self, relation: &Relation) -> Result<Relation, sqlx::Error> {
        sqlx::query_as::<_, Relation>(
            "INSERT INTO relation (name, description) VALUES (?, ?) \
             ON CONFLICT(name) DO UPDATE SET description = excluded.description \
             RETURNING id, name, description",
        )
        .bind(&relation.name)
        .bind(&relation.description)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn get_relation_by_name(&self, name: &str) -> Result<Option<Relation>, sqlx::Error> {
        sqlx::query_as::<_, Relation>("SELECT id, name, description FROM relation WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn list_relations(&self) -> Result<Vec<Relation>, sqlx::Error> {
        sqlx::query_as::<_, Relation>("SELECT id, name, description FROM relation ORDER BY name")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn add_edge(&self, edge: &Edge) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO edge (source, destination, relation_id) VALUES (?, ?, ?) \
             ON CONFLICT DO NOTHING",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.relation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_edge(&self, edge: &Edge) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM edge WHERE source = ? AND destination = ? AND relation_id = ?",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.relation_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn neighbors(&self, source: i64) -> Result<Vec<(Edge, Node)>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT e.source, e.destination, e.relation_id, \
             n.id, n.kind, n.name, n.description, n.content \
             FROM edge e JOIN node n ON n.id = e.destination \
             WHERE e.source = ? ORDER BY e.relation_id, e.destination",
        )
        .bind(source)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| Ok((Edge::from_row(row)?, Node::from_row(row)?)))
            .collect()
    }

    pub async fn search(&self, fts_query: &str) -> Result<Vec<SearchHit>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT n.id, n.kind, n.name, n.description, n.content, bm25(node_fts) AS score \
             FROM node_fts JOIN node n ON n.id = node_fts.rowid \
             WHERE node_fts MATCH ? ORDER BY score, n.id",
        )
        .bind(fts_query)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(SearchHit {
                    node: Node::from_row(row)?,
                    score: row.try_get("score")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> Store {
        let store = Store::in_memory().await.expect("failed to open store");
        store.migrate().await.expect("failed to migrate");
        store
    }

    fn node(kind: &str, name: &str, content: &str) -> Node {
        Node {
            id: 0,
            kind: kind.to_string(),
            name: name.to_string(),
            description: String::new(),
            content: content.to_string(),
        }
    }

    #[tokio::test]
    async fn assigns_id_on_insert() {
        let store = store().await;
        let created = store.upsert_node(&node("person", "Ada", "")).await.unwrap();
        assert!(created.id > 0);
        assert_eq!(store.get_node(created.id).await.unwrap(), Some(created));
    }

    #[tokio::test]
    async fn keeps_id_on_conflict() {
        let store = store().await;
        let created = store
            .upsert_node(&node("person", "Ada", "a"))
            .await
            .unwrap();
        let mut changed = created.clone();
        changed.content = "b".to_string();
        let updated = store.upsert_node(&changed).await.unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.content, "b");
        assert_eq!(store.list_nodes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deletes_node_and_fts_row() {
        let store = store().await;
        let created = store.upsert_node(&node("person", "Ada", "")).await.unwrap();
        assert!(store.delete_node(created.id).await.unwrap());
        assert!(!store.delete_node(created.id).await.unwrap());
        assert!(store.search("ada").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn upsert_relation_is_idempotent_by_name() {
        let store = store().await;
        let relation = Relation {
            id: 0,
            name: "knows".to_string(),
            description: "".to_string(),
        };
        let first = store.upsert_relation(&relation).await.unwrap();
        let mut changed = relation.clone();
        changed.description = "knows someone".to_string();
        let second = store.upsert_relation(&changed).await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.description, "knows someone");
        assert_eq!(store.list_relations().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn traverses_directed_edges() {
        let store = store().await;
        let ada = store.upsert_node(&node("person", "Ada", "")).await.unwrap();
        let bob = store.upsert_node(&node("person", "Bob", "")).await.unwrap();
        let knows = store
            .upsert_relation(&Relation {
                id: 0,
                name: "knows".to_string(),
                description: "".to_string(),
            })
            .await
            .unwrap();
        let edge = Edge {
            source: ada.id,
            destination: bob.id,
            relation_id: knows.id,
        };
        store.add_edge(&edge).await.unwrap();
        store.add_edge(&edge).await.unwrap();
        let neighbors = store.neighbors(ada.id).await.unwrap();
        assert_eq!(neighbors, vec![(edge.clone(), bob.clone())]);
        assert!(store.neighbors(bob.id).await.unwrap().is_empty());
        assert!(store.remove_edge(&edge).await.unwrap());
        assert!(!store.remove_edge(&edge).await.unwrap());
        assert!(store.neighbors(ada.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_matches_and_reflects_updates() {
        let store = store().await;
        let ada = store
            .upsert_node(&node("person", "Ada Lovelace", "pioneer of computing"))
            .await
            .unwrap();
        let hits = store.search("computing").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node.id, ada.id);

        let mut changed = ada.clone();
        changed.content = "mathematician".to_string();
        store.upsert_node(&changed).await.unwrap();
        assert!(store.search("computing").await.unwrap().is_empty());
        assert_eq!(store.search("mathematician").await.unwrap().len(), 1);
    }
}
