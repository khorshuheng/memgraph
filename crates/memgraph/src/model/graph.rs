use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Node {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Edge {
    pub source: i64,
    pub destination: i64,
    pub relation_id: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Relation {
    pub id: i64,
    pub name: String,
    pub description: String,
}
