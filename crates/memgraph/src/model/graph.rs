use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u64,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub source: u64,
    pub destination: u64,
    pub relation_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: u64,
    pub name: String,
    pub description: String,
}
