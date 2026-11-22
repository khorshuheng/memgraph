use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{
    config::PagingConfig,
    model::{
        ChangeDraft, Edge, EdgeFilter, EdgePage, EdgeType, GraphExport, ImportDraft, ImportEdge,
        ImportNode, ImportRef, ImportReport, Neighbor, NeighborDirection, Node, NodeFilter,
        NodeKind, NodeKindProperty, NodePage, NodePatch, NodeRef, NodeSelector, NodeWithMetadata,
        PackDraft, PackEdgeType, PackNodeKind, PackProperty, PackReport, PendingEdge, PendingNode,
        Plan, PlanDraft, PlanTask, PlanTaskDraft, PropertyType, ReachFilter, SearchPage, TagUsage,
        TaskDraft, TaskPatch, WorkDependency, WorkFilter, WorkPage, WorkRef, WorkStatus, WorkTask,
    },
    repository::graph::{GraphRepository, WorkSnapshot},
};

use super::error::ServiceError;

const DEFAULT_DESCEND: &str = "part_of";
const DESCEND_NONE: &str = "none";
const MAX_BATCH_IDS: usize = 200;

#[derive(Clone)]
pub struct GraphService {
    repository: Arc<dyn GraphRepository>,
    paging: PagingConfig,
}

impl GraphService {
    pub fn new(repository: Arc<dyn GraphRepository>, paging: PagingConfig) -> Self {
        Self { repository, paging }
    }

    pub async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, ServiceError> {
        let mut node_kind = node_kind.clone();
        node_kind.name = vocabulary_name(&node_kind.name, "node kind")?;
        self.repository
            .create_node_kind(&node_kind)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_node_kinds(&self) -> Result<Vec<NodeKind>, ServiceError> {
        self.repository
            .list_node_kinds()
            .await
            .map_err(ServiceError::from)
    }

    pub async fn create_edge_type(&self, edge_type: &EdgeType) -> Result<EdgeType, ServiceError> {
        let mut edge_type = edge_type.clone();
        edge_type.name = vocabulary_name(&edge_type.name, "edge type")?;
        self.repository
            .create_edge_type(&edge_type)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_edge_types(&self) -> Result<Vec<EdgeType>, ServiceError> {
        self.repository
            .list_edge_types()
            .await
            .map_err(ServiceError::from)
    }

    pub async fn apply_pack(&self, pack: &PackDraft) -> Result<PackReport, ServiceError> {
        let mut node_kinds = Vec::with_capacity(pack.node_kinds.len());
        for kind in &pack.node_kinds {
            node_kinds.push(PackNodeKind {
                name: vocabulary_name(&kind.name, "node kind")?,
                description: kind.description.clone(),
                unique_name: kind.unique_name,
                strict_properties: kind.strict_properties,
                properties: match &kind.properties {
                    None => None,
                    Some(properties) => Some(normalize_property_declarations(properties)?),
                },
            });
        }

        let mut edge_types = Vec::with_capacity(pack.edge_types.len());
        for edge_type in &pack.edge_types {
            edge_types.push(PackEdgeType {
                name: vocabulary_name(&edge_type.name, "edge type")?,
                description: edge_type.description.clone(),
                resolves: edge_type.resolves,
                single_outgoing: edge_type.single_outgoing,
                acyclic: edge_type.acyclic,
                source_kinds: edge_type
                    .source_kinds
                    .as_deref()
                    .map(normalize_kind_list)
                    .transpose()?,
                destination_kinds: edge_type
                    .destination_kinds
                    .as_deref()
                    .map(normalize_kind_list)
                    .transpose()?,
            });
        }

        reject_duplicate_names(&node_kinds, "node kind", |kind| kind.name.as_str())?;
        reject_duplicate_names(&edge_types, "edge type", |edge_type| {
            edge_type.name.as_str()
        })?;

        let mut known: HashSet<String> = self
            .repository
            .list_node_kinds()
            .await?
            .into_iter()
            .map(|kind| kind.name)
            .collect();
        known.extend(node_kinds.iter().map(|kind| kind.name.clone()));
        for edge_type in &edge_types {
            let referenced = edge_type
                .source_kinds
                .iter()
                .flatten()
                .chain(edge_type.destination_kinds.iter().flatten());
            for kind in referenced {
                if !known.contains(kind) {
                    return Err(ServiceError::UnprocessableEntity(format!(
                        "edge type '{}' references undefined node kind '{kind}' \
                         (kind names are case-sensitive)",
                        edge_type.name
                    )));
                }
            }
        }

        self.repository
            .apply_pack(&PackDraft {
                node_kinds,
                edge_types,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn import_graph(&self, draft: &ImportDraft) -> Result<ImportReport, ServiceError> {
        let mut nodes = Vec::with_capacity(draft.nodes.len());
        let mut references: HashSet<String> = HashSet::new();
        let mut ids: HashSet<i64> = HashSet::new();
        let mut pairs: HashSet<(String, String)> = HashSet::new();
        for node in &draft.nodes {
            let kind = node.kind.trim().to_string();
            if kind.is_empty() {
                return Err(ServiceError::UnprocessableEntity(
                    "node kind must not be empty".to_string(),
                ));
            }
            let name = node_name(&node.name)?;
            let reference = match node.reference.as_deref().map(str::trim) {
                None => None,
                Some("") => {
                    return Err(ServiceError::UnprocessableEntity(
                        "ref must not be empty".to_string(),
                    ));
                }
                Some(reference) => {
                    if !references.insert(reference.to_string()) {
                        return Err(ServiceError::UnprocessableEntity(format!(
                            "ref '{reference}' appears more than once in the import"
                        )));
                    }
                    Some(reference.to_string())
                }
            };
            if let Some(id) = node.id {
                if id <= 0 {
                    return Err(ServiceError::UnprocessableEntity(
                        "import node id must be positive".to_string(),
                    ));
                }
                if !ids.insert(id) {
                    return Err(ServiceError::UnprocessableEntity(format!(
                        "node id {id} appears more than once in the import"
                    )));
                }
            } else if !pairs.insert((kind.clone(), name.clone())) {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "node ({kind}, {name}) appears more than once in the import"
                )));
            }
            nodes.push(ImportNode {
                reference,
                id: node.id,
                kind,
                name,
                description: node.description.clone(),
                content: node.content.clone(),
                tags: match &node.tags {
                    Some(tags) => Some(normalize_tags(tags)?),
                    None => None,
                },
                properties: match &node.properties {
                    Some(properties) if properties.is_object() => Some(properties.clone()),
                    Some(_) => {
                        return Err(ServiceError::UnprocessableEntity(
                            "properties must be a JSON object".to_string(),
                        ));
                    }
                    None => None,
                },
            });
        }

        let mut edges = Vec::with_capacity(draft.edges.len());
        for edge in &draft.edges {
            let edge_type = edge.edge_type.trim().to_string();
            if edge_type.is_empty() {
                return Err(ServiceError::UnprocessableEntity(
                    "edge type must not be empty".to_string(),
                ));
            }
            let properties = match &edge.properties {
                Some(properties) if properties.is_object() => Some(properties.clone()),
                Some(_) => {
                    return Err(ServiceError::UnprocessableEntity(
                        "properties must be a JSON object".to_string(),
                    ));
                }
                None => None,
            };
            edges.push(ImportEdge {
                source: normalize_import_ref(&edge.source)?,
                destination: normalize_import_ref(&edge.destination)?,
                edge_type,
                properties,
            });
        }

        self.repository
            .import_graph(&ImportDraft {
                on_conflict: draft.on_conflict,
                nodes,
                edges,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn upsert_node(&self, node: &Node) -> Result<Node, ServiceError> {
        self.repository
            .upsert_node(node)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn get_node(&self, id: i64) -> Result<Node, ServiceError> {
        match self.repository.get_node(id).await {
            Ok(Some(node)) => Ok(node),
            Ok(None) => Err(ServiceError::EntityNotFound),
            Err(err) => Err(ServiceError::from(err)),
        }
    }

    pub async fn update_node(
        &self,
        node_id: i64,
        patch: &NodePatch,
    ) -> Result<(Node, Vec<String>), ServiceError> {
        let node = self.get_node(node_id).await?;
        if patch.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "provide at least one of name, description, content or tags".to_string(),
            ));
        }
        let mut normalized = patch.clone();
        if let Some(name) = &normalized.name {
            normalized.name = Some(node_name(name)?);
        }
        if let Some(tags) = &normalized.tags {
            normalized.tags = Some(normalize_tags(tags)?);
        }
        if let Some(properties) = &normalized.properties
            && !properties.is_object()
        {
            return Err(ServiceError::UnprocessableEntity(
                "properties must be a JSON object".to_string(),
            ));
        }
        if let Some(name) = &normalized.name {
            self.require_available_name(node.kind_id, name, node_id)
                .await?;
        }
        self.repository
            .update_node(node_id, &normalized)
            .await
            .map_err(Into::into)
    }

    pub async fn delete_node(&self, id: i64) -> Result<(), ServiceError> {
        match self.repository.delete_node(id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ServiceError::EntityNotFound),
            Err(err) => Err(ServiceError::from(err)),
        }
    }

    pub async fn list_nodes(&self, filter: &NodeFilter) -> Result<Vec<Node>, ServiceError> {
        let filter = self.normalize_node_filter(filter).await?;
        self.repository
            .list_nodes(&filter)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_nodes_page(
        &self,
        filter: &NodeFilter,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<NodePage, ServiceError> {
        let filter = self.normalize_node_filter(filter).await?;
        let limit = limit
            .unwrap_or(self.paging.default_limit)
            .clamp(1, self.paging.max_limit);
        let offset = offset.unwrap_or(0).max(0);
        let (items, total) = self
            .repository
            .list_nodes_page(&filter, limit, offset)
            .await?;
        Ok(NodePage {
            items,
            total,
            limit,
            offset,
        })
    }

    async fn normalize_node_filter(&self, filter: &NodeFilter) -> Result<NodeFilter, ServiceError> {
        if let Some(kind) = &filter.kind {
            let exists = self
                .repository
                .list_node_kinds()
                .await?
                .iter()
                .any(|node_kind| &node_kind.name == kind);
            if !exists {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "node kind '{kind}' is not defined"
                )));
            }
        }
        Ok(NodeFilter {
            kind: filter.kind.clone(),
            name: filter.name.clone(),
            name_contains: filter.name_contains.clone(),
            scope: filter.scope,
            reach: self.normalize_reach(&filter.reach).await?,
            properties: filter.properties.clone(),
        })
    }

    async fn normalize_reach(&self, reach: &ReachFilter) -> Result<ReachFilter, ServiceError> {
        if reach.within.is_none() {
            if reach.via.is_some() || reach.descend.is_some() {
                return Err(ServiceError::UnprocessableEntity(
                    "via and descend require within".to_string(),
                ));
            }
            return Ok(ReachFilter::default());
        }
        let within = reach.within.expect("within is present");
        if self.repository.get_node(within).await?.is_none() {
            return Err(ServiceError::UnprocessableEntity(format!(
                "within references unknown node {within}"
            )));
        }
        let edge_types = self.repository.list_edge_types().await?;
        if let Some(via) = &reach.via
            && !edge_types.iter().any(|edge_type| &edge_type.name == via)
        {
            return Err(ServiceError::UnprocessableEntity(format!(
                "edge type '{via}' is not defined"
            )));
        }
        let descend = match reach.descend.as_deref() {
            None => Some(DEFAULT_DESCEND.to_string()),
            Some(DESCEND_NONE) => None,
            Some(name) => {
                if !edge_types.iter().any(|edge_type| edge_type.name == name) {
                    return Err(ServiceError::UnprocessableEntity(format!(
                        "edge type '{name}' is not defined"
                    )));
                }
                Some(name.to_string())
            }
        };
        Ok(ReachFilter {
            within: Some(within),
            via: reach.via.clone(),
            descend,
        })
    }

    pub async fn add_edge(&self, edge: &Edge) -> Result<(), ServiceError> {
        self.add_edge_with_properties(edge, None).await
    }

    pub async fn add_edge_with_properties(
        &self,
        edge: &Edge,
        properties: Option<&serde_json::Value>,
    ) -> Result<(), ServiceError> {
        let encoded = encode_properties(properties)?;
        let edge_types = self.repository.list_edge_types().await?;
        let edge_type = edge_types
            .iter()
            .find(|edge_type| edge_type.id == edge.edge_type_id)
            .ok_or_else(|| {
                ServiceError::UnprocessableEntity(format!(
                    "edge type {} is not defined",
                    edge.edge_type_id
                ))
            })?;
        let kinds = self
            .node_kind_names(&[edge.source, edge.destination])
            .await?;
        require_kind(
            &kinds,
            edge.source,
            &edge_type.source_kind_list(),
            &format!("edge type '{}' source", edge_type.name),
        )?;
        require_kind(
            &kinds,
            edge.destination,
            &edge_type.destination_kind_list(),
            &format!("edge type '{}' destination", edge_type.name),
        )?;
        self.require_single_outgoing(edge.source, edge.destination, edge_type)
            .await?;
        self.repository
            .add_edge_with_properties(edge, encoded.as_deref())
            .await
            .map_err(ServiceError::from)
    }

    pub async fn set_edge_properties(
        &self,
        edge: &Edge,
        properties: &serde_json::Value,
    ) -> Result<(), ServiceError> {
        let encoded = encode_properties(Some(properties))?.ok_or_else(|| {
            ServiceError::UnprocessableEntity("properties must be a JSON object".to_string())
        })?;
        let updated = self.repository.set_edge_properties(edge, &encoded).await?;
        if updated {
            Ok(())
        } else {
            Err(ServiceError::EntityNotFound)
        }
    }

    async fn require_single_outgoing(
        &self,
        source: i64,
        destination: i64,
        edge_type: &EdgeType,
    ) -> Result<(), ServiceError> {
        if !edge_type.single_outgoing {
            return Ok(());
        }
        let targets = self
            .repository
            .outgoing_edge_targets(source, edge_type.id)
            .await?;
        if targets.iter().any(|target| *target != destination) {
            return Err(ServiceError::UnprocessableEntity(format!(
                "edge type '{}' allows one outgoing edge, and node {source} already points at {}",
                edge_type.name,
                targets
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            )));
        }
        Ok(())
    }

    pub async fn remove_edge(&self, edge: &Edge) -> Result<(), ServiceError> {
        match self.repository.remove_edge(edge).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ServiceError::EntityNotFound),
            Err(err) => Err(ServiceError::from(err)),
        }
    }

    pub async fn neighbors(
        &self,
        node_id: i64,
        direction: NeighborDirection,
        edge_type: Option<&str>,
    ) -> Result<Vec<Neighbor>, ServiceError> {
        if let Some(name) = edge_type {
            let exists = self
                .repository
                .list_edge_types()
                .await?
                .iter()
                .any(|edge_type| edge_type.name == name);
            if !exists {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "edge type '{name}' is not defined"
                )));
            }
        }
        self.repository
            .neighbors(node_id, direction, edge_type)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn update_task(&self, task_id: i64, patch: &TaskPatch) -> Result<Node, ServiceError> {
        if patch.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "provide at least one of name, description, content or tags".to_string(),
            ));
        }
        let node = self.get_node(task_id).await?;
        let task_kind = self.kind_id("task").await?;
        if node.kind_id != task_kind {
            return Err(ServiceError::UnprocessableEntity(format!(
                "node {task_id} is not a task"
            )));
        }
        let (updated, _) = self
            .update_node(
                task_id,
                &NodePatch {
                    name: patch.name.clone(),
                    description: patch.description.clone(),
                    content: patch.content.clone(),
                    tags: patch.tags.clone(),
                    properties: patch.properties.clone(),
                },
            )
            .await?;
        Ok(updated)
    }

    pub async fn create_task(&self, draft: &TaskDraft) -> Result<Node, ServiceError> {
        let mut normalized = draft.clone();
        normalized.name = node_name(&normalized.name)?;
        if let Some(tags) = &draft.tags {
            normalized.tags = Some(normalize_tags(tags)?);
        }
        let draft = &normalized;
        if draft.id < 0 {
            return Err(ServiceError::UnprocessableEntity(format!(
                "task has invalid node id {}",
                draft.id
            )));
        }
        let Some(parent) = draft.parent else {
            return Err(ServiceError::UnprocessableEntity(
                "a task requires a parent: provide the goal or task it belongs to".to_string(),
            ));
        };
        let task_kind = self.kind_id("task").await?;
        self.require_available_name(task_kind, &draft.name, draft.id)
            .await?;
        let edge_types = self.repository.list_edge_types().await?;
        let PlanEdgeTypes {
            part_of,
            depends_on,
            affects,
        } = plan_edge_types(&edge_types)?;

        let mut referenced = Vec::new();
        if let Some(parent) = draft.parent {
            referenced.push(parent);
        }
        referenced.extend(draft.depends_on.iter().copied());
        referenced.extend(draft.affects.iter().copied());
        let kinds = self.node_kind_names(&referenced).await?;
        require_kind(
            &kinds,
            parent,
            &part_of.destination_kind_list(),
            "part_of parent",
        )?;
        require_kinds_for(
            &kinds,
            &draft.depends_on,
            &depends_on.destination_kind_list(),
            "depends_on target",
        )?;
        require_kinds_for(
            &kinds,
            &draft.affects,
            &affects.destination_kind_list(),
            "affects target",
        )?;
        if !self.repository.is_anchored(parent).await? {
            return Err(ServiceError::UnprocessableEntity(format!(
                "part_of parent references node {parent}, which is not attached to a repo via belongs_to"
            )));
        }

        let node = Node {
            id: draft.id,
            kind_id: task_kind,
            name: draft.name.clone(),
            description: draft.description.clone(),
            content: draft.content.clone(),
            updated_at: String::new(),
        };
        let mut edges = Vec::new();
        edges.push(PendingEdge {
            source: NodeRef::New(0),
            destination: NodeRef::Existing(parent),
            edge_type_id: part_of.id,
        });
        for dependency in &draft.depends_on {
            edges.push(PendingEdge {
                source: NodeRef::New(0),
                destination: NodeRef::Existing(*dependency),
                edge_type_id: depends_on.id,
            });
        }
        for target in &draft.affects {
            edges.push(PendingEdge {
                source: NodeRef::New(0),
                destination: NodeRef::Existing(*target),
                edge_type_id: affects.id,
            });
        }

        let pending = PendingNode {
            node,
            properties: draft.properties.clone(),
            tags: draft.tags.clone(),
        };
        let mut created = self
            .repository
            .insert_nodes_with_edges(std::slice::from_ref(&pending), &edges)
            .await?;
        created.pop().ok_or_else(|| {
            ServiceError::InternalError("task creation returned no node".to_string())
        })
    }

    pub async fn create_plan(&self, draft: &PlanDraft) -> Result<Plan, ServiceError> {
        let mut normalized = draft.clone();
        if let Some(goal) = &mut normalized.goal {
            goal.name = node_name(&goal.name)?;
            if let Some(tags) = &goal.tags {
                goal.tags = Some(normalize_tags(tags)?);
            }
        }
        for task in &mut normalized.tasks {
            task.name = node_name(&task.name)?;
            if let Some(tags) = &task.tags {
                task.tags = Some(normalize_tags(tags)?);
            }
        }
        let draft = &normalized;

        validate_plan_root(draft)?;
        validate_supplied_ids(draft)?;

        let node_kinds = self.repository.list_node_kinds().await?;
        let task_kind = kind_id_of(&node_kinds, "task")?;
        let goal_kind = if draft.goal.is_some() {
            Some(kind_id_of(&node_kinds, "goal")?)
        } else {
            None
        };

        if self.repository.node_kind_unique_name(task_kind).await? {
            let mut task_names: HashSet<&str> = HashSet::new();
            for task in &draft.tasks {
                if !task_names.insert(task.name.as_str()) {
                    return Err(ServiceError::UnprocessableEntity(format!(
                        "two tasks share the name '{}'; node names are unique per kind",
                        task.name
                    )));
                }
            }
        }
        let edge_types = self.repository.list_edge_types().await?;
        let types = plan_edge_types(&edge_types)?;

        let kinds = self.validate_plan_references(draft, types.affects).await?;
        let anchor_edge = self.resolve_anchor_edge(draft, &edge_types, &kinds).await?;
        let (key_index, offset) = build_key_index(draft)?;
        validate_key_references(draft, &key_index)?;
        assert_acyclic_parents(&draft.tasks)?;
        assert_acyclic_dependencies(&draft.tasks)?;

        if let (Some(goal), Some(goal_kind)) = (&draft.goal, goal_kind) {
            self.require_available_name(goal_kind, &goal.name, goal.id)
                .await?;
        }
        for task in &draft.tasks {
            self.require_available_name(task_kind, &task.name, task.id)
                .await?;
        }

        let nodes = build_plan_nodes(draft, task_kind, goal_kind, offset);
        let root = match draft.goal_id {
            Some(goal_id) => NodeRef::Existing(goal_id),
            None => NodeRef::New(0),
        };
        let edges = build_plan_edges(draft, &key_index, root, anchor_edge, &types, offset)?;

        let created = self
            .repository
            .insert_nodes_with_edges(&nodes, &edges)
            .await?;
        Ok(assemble_plan(draft, created))
    }

    pub async fn create_change(&self, draft: &ChangeDraft) -> Result<Node, ServiceError> {
        let commit = node_name(&draft.commit)?;
        let title = draft.title.trim();
        if title.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "title must not be empty".to_string(),
            ));
        }
        let mut tasks: Vec<i64> = Vec::new();
        for selector in &draft.tasks {
            let task = self.resolve_node_selector(selector).await?;
            if !tasks.contains(&task) {
                tasks.push(task);
            }
        }
        let mut files: Vec<String> = Vec::new();
        for file in &draft.files {
            let file = file.trim();
            if file.is_empty() {
                return Err(ServiceError::UnprocessableEntity(
                    "file name must not be empty".to_string(),
                ));
            }
            if !files.iter().any(|existing| existing == file) {
                files.push(file.to_string());
            }
        }
        if tasks.is_empty() && files.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "provide at least one task or file for the change".to_string(),
            ));
        }

        let change_kind = self.kind_id("change").await?;
        if let Some(existing) = self
            .repository
            .node_id_by_natural_key(change_kind, &commit)
            .await?
        {
            return Err(ServiceError::Conflict(format!(
                "a change named '{commit}' already exists with id {existing}"
            )));
        }

        let kinds = self.node_kind_names(&tasks).await?;
        for task in &tasks {
            let kind = kinds.get(task).ok_or(ServiceError::EntityNotFound)?;
            if kind != "task" {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "node {task} is not a task"
                )));
            }
        }

        let file_kind = self.kind_id("file").await?;
        let mut file_ids = Vec::new();
        for file in &files {
            let id = self
                .repository
                .node_id_by_natural_key(file_kind, file)
                .await?
                .ok_or_else(|| {
                    ServiceError::UnprocessableEntity(format!("file '{file}' is not in the graph"))
                })?;
            file_ids.push(id);
        }

        let edge_types = self.repository.list_edge_types().await?;
        let implements = edge_type_ref(&edge_types, "implements")?;
        let modifies = edge_type_ref(&edge_types, "modifies")?;

        let tags = match &draft.tags {
            Some(tags) => Some(normalize_tags(tags)?),
            None => None,
        };
        let node = Node {
            id: 0,
            kind_id: change_kind,
            name: commit,
            description: title.to_string(),
            content: draft.summary.clone(),
            updated_at: String::new(),
        };
        let pending = PendingNode {
            node,
            properties: draft.properties.clone(),
            tags,
        };
        let mut edges = Vec::new();
        for task in &tasks {
            edges.push(PendingEdge {
                source: NodeRef::New(0),
                destination: NodeRef::Existing(*task),
                edge_type_id: implements.id,
            });
        }
        for id in file_ids {
            edges.push(PendingEdge {
                source: NodeRef::New(0),
                destination: NodeRef::Existing(id),
                edge_type_id: modifies.id,
            });
        }
        let mut created = self
            .repository
            .insert_nodes_with_edges(std::slice::from_ref(&pending), &edges)
            .await?;
        created.pop().ok_or_else(|| {
            ServiceError::InternalError("change creation returned no node".to_string())
        })
    }

    async fn validate_plan_references(
        &self,
        draft: &PlanDraft,
        affects: &EdgeType,
    ) -> Result<HashMap<i64, String>, ServiceError> {
        let mut referenced = Vec::new();
        if let Some(goal_id) = draft.goal_id {
            referenced.push(goal_id);
        }
        if let Some(anchor) = &draft.anchor {
            referenced.push(anchor.node_id);
        }
        for task in &draft.tasks {
            referenced.extend(task.affects.iter().copied());
        }
        let kinds = self.node_kind_names(&referenced).await?;
        if let Some(goal_id) = draft.goal_id {
            require_kind(&kinds, goal_id, &["goal"], "goal_id")?;
            if draft.anchor.is_none() && !self.repository.is_anchored(goal_id).await? {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "goal_id references node {goal_id}, which is not attached to a repo; provide anchor"
                )));
            }
        }
        let allowed = affects.destination_kind_list();
        for task in &draft.tasks {
            require_kinds_for(
                &kinds,
                &task.affects,
                &allowed,
                &format!("plan task '{}' affects", task.key),
            )?;
        }
        Ok(kinds)
    }

    async fn resolve_anchor_edge(
        &self,
        draft: &PlanDraft,
        edge_types: &[EdgeType],
        kinds: &HashMap<i64, String>,
    ) -> Result<Option<(i64, i64)>, ServiceError> {
        let Some(anchor) = &draft.anchor else {
            return Ok(None);
        };
        let edge_type = edge_type_ref(edge_types, &anchor.edge_type)?;
        let source_kinds = edge_type.source_kind_list();
        if !source_kinds.is_empty() && !source_kinds.contains(&"goal") {
            return Err(ServiceError::UnprocessableEntity(format!(
                "edge type '{}' does not accept a goal source",
                edge_type.name
            )));
        }
        require_kind(
            kinds,
            anchor.node_id,
            &edge_type.destination_kind_list(),
            &format!("plan anchor edge '{}' destination", edge_type.name),
        )?;
        if let Some(goal_id) = draft.goal_id {
            self.require_single_outgoing(goal_id, anchor.node_id, edge_type)
                .await?;
        }
        Ok(Some((edge_type.id, anchor.node_id)))
    }

    async fn node_kind_names(&self, ids: &[i64]) -> Result<HashMap<i64, String>, ServiceError> {
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            if seen.insert(*id) {
                unique.push(*id);
            }
        }
        let rows = self.repository.node_kinds_for_ids(&unique).await?;
        Ok(rows.into_iter().collect())
    }

    async fn require_available_name(
        &self,
        kind_id: i64,
        name: &str,
        own_id: i64,
    ) -> Result<(), ServiceError> {
        if !self.repository.node_kind_unique_name(kind_id).await? {
            return Ok(());
        }
        if let Some(existing) = self
            .repository
            .node_id_by_natural_key(kind_id, name)
            .await?
            && existing != own_id
        {
            return Err(ServiceError::Conflict(format!(
                "a node named '{name}' already exists with id {existing}; \
                 node names are unique per kind"
            )));
        }
        Ok(())
    }

    async fn kind_id(&self, name: &str) -> Result<i64, ServiceError> {
        kind_id_of(&self.repository.list_node_kinds().await?, name)
    }

    pub async fn search(
        &self,
        query: Option<&str>,
        tags: &[String],
        properties: &serde_json::Map<String, serde_json::Value>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<SearchPage, ServiceError> {
        let query = query.map(str::trim).filter(|query| !query.is_empty());
        let tags = normalize_tags(tags)?;
        if query.is_none() && tags.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "provide q or at least one tag".to_string(),
            ));
        }
        let fts_query = query.and_then(fts_query);
        if fts_query.is_none() && tags.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "q does not contain any searchable term".to_string(),
            ));
        }
        let limit = limit
            .unwrap_or(self.paging.default_limit)
            .clamp(1, self.paging.max_limit);
        let offset = offset.unwrap_or(0).max(0);
        self.repository
            .search_nodes(fts_query.as_deref(), &tags, properties, limit, offset)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn create_node(
        &self,
        node: &Node,
        tags: &[String],
    ) -> Result<(Node, Vec<String>), ServiceError> {
        self.create_node_with_properties(node, tags, None).await
    }

    pub async fn create_node_with_properties(
        &self,
        node: &Node,
        tags: &[String],
        properties: Option<&serde_json::Value>,
    ) -> Result<(Node, Vec<String>), ServiceError> {
        let mut node = node.clone();
        node.name = node_name(&node.name)?;
        self.require_available_name(node.kind_id, &node.name, 0)
            .await?;
        let tags = normalize_tags(tags)?;
        encode_properties(properties)?;
        let created = self
            .repository
            .upsert_node_with_tags(&node, &tags, properties)
            .await?;
        Ok((created, tags))
    }

    pub async fn node_properties(&self, node_id: i64) -> Result<serde_json::Value, ServiceError> {
        let raw = self.repository.node_properties(node_id).await?;
        Ok(parse_properties(&raw))
    }

    pub async fn node_kind_properties_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<NodeKindProperty>>, ServiceError> {
        self.repository
            .node_kind_properties_for_ids(ids)
            .await
            .map_err(Into::into)
    }

    pub async fn node_properties_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, serde_json::Value>, ServiceError> {
        let raw = self.repository.node_properties_for_ids(ids).await?;
        Ok(raw
            .into_iter()
            .map(|(node_id, properties)| (node_id, parse_properties(&properties)))
            .collect())
    }

    pub async fn node_tags(&self, node_id: i64) -> Result<Vec<String>, ServiceError> {
        self.get_node(node_id).await?;
        self.repository.node_tags(node_id).await.map_err(Into::into)
    }

    pub async fn node_tags_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<String>>, ServiceError> {
        self.repository
            .node_tags_for_ids(ids)
            .await
            .map_err(Into::into)
    }

    pub async fn replace_node_tags(
        &self,
        node_id: i64,
        tags: &[String],
    ) -> Result<Vec<String>, ServiceError> {
        self.get_node(node_id).await?;
        let tags = normalize_tags(tags)?;
        self.repository.set_node_tags(node_id, &tags).await?;
        Ok(tags)
    }

    pub async fn list_tags(&self) -> Result<Vec<TagUsage>, ServiceError> {
        self.repository.list_tags().await.map_err(Into::into)
    }

    pub async fn nodes_by_ids(&self, ids: &[i64]) -> Result<Vec<NodeWithMetadata>, ServiceError> {
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            if seen.insert(*id) {
                unique.push(*id);
            }
        }
        if unique.len() > MAX_BATCH_IDS {
            return Err(ServiceError::UnprocessableEntity(format!(
                "at most {MAX_BATCH_IDS} ids are allowed"
            )));
        }
        let snapshot = self.repository.nodes_snapshot(&unique).await?;
        let mut by_id: HashMap<i64, Node> = snapshot
            .nodes
            .into_iter()
            .map(|node| (node.id, node))
            .collect();
        let mut tags = snapshot.tags;
        let mut properties = snapshot.properties;
        Ok(unique
            .into_iter()
            .filter_map(|id| {
                let node = by_id.remove(&id)?;
                let tags = tags.remove(&id).unwrap_or_default();
                let raw = properties.remove(&id).unwrap_or_else(|| "{}".to_string());
                let properties =
                    serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
                Some(NodeWithMetadata {
                    node,
                    tags,
                    properties,
                })
            })
            .collect())
    }

    pub async fn resolve_node_selector(
        &self,
        selector: &NodeSelector,
    ) -> Result<i64, ServiceError> {
        match selector {
            NodeSelector::Id(id) => Ok(*id),
            NodeSelector::Named { kind, name } => {
                let kind_id = self.kind_id(kind).await?;
                let ids = self.repository.node_ids_by_name(kind_id, name).await?;
                match ids.as_slice() {
                    [] => Err(ServiceError::UnprocessableEntity(format!(
                        "no {kind} named '{name}'"
                    ))),
                    [id] => Ok(*id),
                    _ => Err(ServiceError::UnprocessableEntity(format!(
                        "{kind} name '{name}' is ambiguous; use an id"
                    ))),
                }
            }
        }
    }

    pub async fn resolve_edge_type(
        &self,
        edge_type_id: Option<i64>,
        edge_type: Option<&str>,
    ) -> Result<i64, ServiceError> {
        match (edge_type_id, edge_type) {
            (Some(id), None) => Ok(id),
            (None, Some(name)) => self
                .repository
                .list_edge_types()
                .await?
                .into_iter()
                .find(|edge_type| edge_type.name == name)
                .map(|edge_type| edge_type.id)
                .ok_or_else(|| {
                    ServiceError::UnprocessableEntity(format!("edge type '{name}' is not defined"))
                }),
            (None, None) => Err(ServiceError::UnprocessableEntity(
                "provide edge_type_id or edge_type".to_string(),
            )),
            (Some(_), Some(_)) => Err(ServiceError::UnprocessableEntity(
                "provide exactly one of edge_type_id or edge_type".to_string(),
            )),
        }
    }

    pub async fn name_maps(
        &self,
    ) -> Result<(HashMap<i64, String>, HashMap<i64, String>), ServiceError> {
        let kinds = self
            .repository
            .list_node_kinds()
            .await?
            .into_iter()
            .map(|kind| (kind.id, kind.name))
            .collect();
        let edge_types = self
            .repository
            .list_edge_types()
            .await?
            .into_iter()
            .map(|edge_type| (edge_type.id, edge_type.name))
            .collect();
        Ok((kinds, edge_types))
    }

    pub async fn list_edges(
        &self,
        filter: &EdgeFilter,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<EdgePage, ServiceError> {
        if let Some(name) = &filter.edge_type {
            let exists = self
                .repository
                .list_edge_types()
                .await?
                .iter()
                .any(|edge_type| &edge_type.name == name);
            if !exists {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "edge type '{name}' is not defined"
                )));
            }
        }
        let limit = limit
            .unwrap_or(self.paging.default_limit)
            .clamp(1, self.paging.max_limit);
        let offset = offset.unwrap_or(0).max(0);
        let (items, total) = self.repository.list_edges(filter, limit, offset).await?;
        Ok(EdgePage {
            items,
            total,
            limit,
            offset,
        })
    }

    pub async fn export_graph(&self) -> Result<GraphExport, ServiceError> {
        self.repository.graph_export().await.map_err(Into::into)
    }

    pub async fn work_report(
        &self,
        filter: &WorkFilter,
        only_ready: bool,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<WorkPage, ServiceError> {
        self.validate_work_filter(filter).await?;
        let (limit, offset) = self.work_page(limit, offset);
        let snapshot = self
            .repository
            .work_snapshot(filter, only_ready, false, limit, offset)
            .await?;
        Ok(assemble_work_page(snapshot))
    }

    pub async fn goal_tree(
        &self,
        goal_id: i64,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<(Node, WorkPage), ServiceError> {
        let goal = self.get_node(goal_id).await?;
        let goal_kind = self.kind_id("goal").await?;
        if goal.kind_id != goal_kind {
            return Err(ServiceError::UnprocessableEntity(format!(
                "node {goal_id} is not a goal"
            )));
        }
        let (limit, offset) = self.work_page(limit, offset);
        let snapshot = self
            .repository
            .work_snapshot(
                &WorkFilter {
                    goal: Some(goal_id),
                    repo: None,
                },
                false,
                true,
                limit,
                offset,
            )
            .await?;
        Ok((goal, assemble_work_page(snapshot)))
    }

    fn work_page(&self, limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
        (
            limit
                .unwrap_or(self.paging.default_limit)
                .clamp(1, self.paging.max_limit),
            offset.unwrap_or(0).max(0),
        )
    }

    async fn validate_work_filter(&self, filter: &WorkFilter) -> Result<(), ServiceError> {
        if let Some(goal) = filter.goal {
            let node = self.get_node(goal).await?;
            let goal_kind = self.kind_id("goal").await?;
            if node.kind_id != goal_kind {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "node {goal} is not a goal"
                )));
            }
        }
        if let Some(repo) = filter.repo {
            let node = self.get_node(repo).await?;
            let repo_kind = self.kind_id("repo").await?;
            if node.kind_id != repo_kind {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "node {repo} is not a repo"
                )));
            }
        }
        Ok(())
    }
}

fn assemble_work_page(snapshot: WorkSnapshot) -> WorkPage {
    let mut deps_by_task: HashMap<i64, Vec<WorkDependency>> = HashMap::new();
    for dependency in snapshot.dependencies {
        deps_by_task
            .entry(dependency.task_id)
            .or_default()
            .push(WorkDependency {
                id: dependency.dependency_id,
                name: dependency.dependency_name,
                done: dependency.done,
            });
    }
    let mut impls_by_task: HashMap<i64, Vec<WorkRef>> = HashMap::new();
    for implementation in snapshot.implementations {
        impls_by_task
            .entry(implementation.task_id)
            .or_default()
            .push(WorkRef {
                id: implementation.change_id,
                name: implementation.change_name,
            });
    }
    let mut items = Vec::with_capacity(snapshot.rows.len());
    for row in snapshot.rows {
        let status = if row.done {
            WorkStatus::Done
        } else if row.blocked {
            WorkStatus::Blocked
        } else {
            WorkStatus::Pending
        };
        items.push(WorkTask {
            id: row.id,
            name: row.name,
            status,
            updated_at: row.updated_at,
            goal_id: row.goal_id,
            goal_name: row.goal_name,
            depends_on: deps_by_task.remove(&row.id).unwrap_or_default(),
            implemented_by: impls_by_task.remove(&row.id).unwrap_or_default(),
        });
    }
    WorkPage {
        items,
        total: snapshot.total,
        limit: snapshot.limit,
        offset: snapshot.offset,
    }
}

fn edge_type_ref<'a>(edge_types: &'a [EdgeType], name: &str) -> Result<&'a EdgeType, ServiceError> {
    edge_types
        .iter()
        .find(|edge_type| edge_type.name == name)
        .ok_or_else(|| {
            ServiceError::UnprocessableEntity(format!("edge type '{name}' is not defined"))
        })
}

fn validate_plan_root(draft: &PlanDraft) -> Result<(), ServiceError> {
    match (&draft.goal, draft.goal_id) {
        (Some(_), Some(_)) => {
            return Err(ServiceError::UnprocessableEntity(
                "provide either goal or goal_id, not both".to_string(),
            ));
        }
        (None, None) => {
            return Err(ServiceError::UnprocessableEntity(
                "a plan requires a root: provide goal or goal_id".to_string(),
            ));
        }
        _ => {}
    }
    if draft.goal.is_some() && draft.anchor.is_none() {
        return Err(ServiceError::UnprocessableEntity(
            "a plan requires an anchor: provide anchor to attach the goal to a repo".to_string(),
        ));
    }
    Ok(())
}

fn validate_supplied_ids(draft: &PlanDraft) -> Result<(), ServiceError> {
    let mut supplied_ids: HashSet<i64> = HashSet::new();
    if let Some(goal) = &draft.goal {
        validate_supplied_id(goal.id, "plan goal", &mut supplied_ids)?;
    }
    for task in &draft.tasks {
        validate_supplied_id(
            task.id,
            &format!("plan task '{}'", task.key),
            &mut supplied_ids,
        )?;
    }
    Ok(())
}

fn build_key_index(draft: &PlanDraft) -> Result<(HashMap<&str, usize>, usize), ServiceError> {
    let offset = if draft.goal.is_some() { 1 } else { 0 };
    let mut key_index: HashMap<&str, usize> = HashMap::new();
    for (position, task) in draft.tasks.iter().enumerate() {
        if task.key.trim().is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "every plan task needs a non-empty key".to_string(),
            ));
        }
        if key_index
            .insert(task.key.as_str(), offset + position)
            .is_some()
        {
            return Err(ServiceError::UnprocessableEntity(format!(
                "duplicate plan task key '{}'",
                task.key
            )));
        }
    }
    Ok((key_index, offset))
}

fn validate_key_references(
    draft: &PlanDraft,
    key_index: &HashMap<&str, usize>,
) -> Result<(), ServiceError> {
    for task in &draft.tasks {
        if let Some(parent) = &task.parent {
            if parent == &task.key {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "plan task '{}' cannot be its own parent",
                    task.key
                )));
            }
            if !key_index.contains_key(parent.as_str()) {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "plan task '{}' references unknown parent '{}'",
                    task.key, parent
                )));
            }
        }
        for dependency in &task.depends_on {
            if dependency == &task.key {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "plan task '{}' cannot depend on itself",
                    task.key
                )));
            }
            if !key_index.contains_key(dependency.as_str()) {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "plan task '{}' references unknown dependency '{}'",
                    task.key, dependency
                )));
            }
        }
    }
    Ok(())
}

fn build_plan_nodes(
    draft: &PlanDraft,
    task_kind: i64,
    goal_kind: Option<i64>,
    offset: usize,
) -> Vec<PendingNode> {
    let mut nodes = Vec::with_capacity(offset + draft.tasks.len());
    if let (Some(goal), Some(goal_kind)) = (&draft.goal, goal_kind) {
        nodes.push(PendingNode {
            node: Node {
                id: goal.id,
                kind_id: goal_kind,
                name: goal.name.clone(),
                description: goal.description.clone(),
                content: goal.content.clone(),
                updated_at: String::new(),
            },
            properties: goal.properties.clone(),
            tags: goal.tags.clone(),
        });
    }
    for task in &draft.tasks {
        nodes.push(PendingNode {
            node: Node {
                id: task.id,
                kind_id: task_kind,
                name: task.name.clone(),
                description: task.description.clone(),
                content: task.content.clone(),
                updated_at: String::new(),
            },
            properties: task.properties.clone(),
            tags: task.tags.clone(),
        });
    }
    nodes
}

fn build_plan_edges<'a>(
    draft: &'a PlanDraft,
    key_index: &HashMap<&'a str, usize>,
    root: NodeRef,
    anchor_edge: Option<(i64, i64)>,
    types: &PlanEdgeTypes<'_>,
    offset: usize,
) -> Result<Vec<PendingEdge>, ServiceError> {
    let mut edges = Vec::new();
    if let Some((edge_type_id, node_id)) = anchor_edge {
        edges.push(PendingEdge {
            source: root,
            destination: NodeRef::Existing(node_id),
            edge_type_id,
        });
    }
    for (position, task) in draft.tasks.iter().enumerate() {
        let task_ref = NodeRef::New(offset + position);
        let parent = match &task.parent {
            Some(parent) => NodeRef::New(*key_index.get(parent.as_str()).ok_or_else(|| {
                ServiceError::UnprocessableEntity(format!(
                    "plan task '{}' references unknown parent '{}'",
                    task.key, parent
                ))
            })?),
            None => root,
        };
        edges.push(PendingEdge {
            source: task_ref,
            destination: parent,
            edge_type_id: types.part_of.id,
        });
        for dependency in &task.depends_on {
            let position = *key_index.get(dependency.as_str()).ok_or_else(|| {
                ServiceError::UnprocessableEntity(format!(
                    "plan task '{}' references unknown dependency '{}'",
                    task.key, dependency
                ))
            })?;
            edges.push(PendingEdge {
                source: task_ref,
                destination: NodeRef::New(position),
                edge_type_id: types.depends_on.id,
            });
        }
        for target in &task.affects {
            edges.push(PendingEdge {
                source: task_ref,
                destination: NodeRef::Existing(*target),
                edge_type_id: types.affects.id,
            });
        }
    }
    Ok(edges)
}

fn assemble_plan(draft: &PlanDraft, created: Vec<Node>) -> Plan {
    let mut created = created.into_iter();
    let goal = draft.goal.as_ref().and_then(|_| created.next());
    let tasks = draft
        .tasks
        .iter()
        .zip(created)
        .map(|(draft_task, node)| PlanTask {
            key: draft_task.key.clone(),
            node,
        })
        .collect();
    Plan { goal, tasks }
}

fn kind_id_of(kinds: &[NodeKind], name: &str) -> Result<i64, ServiceError> {
    kinds
        .iter()
        .find(|kind| kind.name == name)
        .map(|kind| kind.id)
        .ok_or_else(|| {
            ServiceError::UnprocessableEntity(format!("node kind '{name}' is not defined"))
        })
}

struct PlanEdgeTypes<'a> {
    part_of: &'a EdgeType,
    depends_on: &'a EdgeType,
    affects: &'a EdgeType,
}

fn plan_edge_types(edge_types: &[EdgeType]) -> Result<PlanEdgeTypes<'_>, ServiceError> {
    Ok(PlanEdgeTypes {
        part_of: edge_type_ref(edge_types, "part_of")?,
        depends_on: edge_type_ref(edge_types, "depends_on")?,
        affects: edge_type_ref(edge_types, "affects")?,
    })
}

fn require_kinds_for(
    kinds: &HashMap<i64, String>,
    ids: &[i64],
    allowed: &[&str],
    context: &str,
) -> Result<(), ServiceError> {
    for id in ids {
        require_kind(kinds, *id, allowed, context)?;
    }
    Ok(())
}

fn require_kind(
    kinds: &HashMap<i64, String>,
    id: i64,
    allowed: &[&str],
    context: &str,
) -> Result<(), ServiceError> {
    match kinds.get(&id) {
        None => Err(ServiceError::UnprocessableEntity(format!(
            "{context} references unknown node {id}"
        ))),
        Some(_) if allowed.is_empty() => Ok(()),
        Some(kind) if allowed.contains(&kind.as_str()) => Ok(()),
        Some(kind) => Err(ServiceError::UnprocessableEntity(format!(
            "{context} references node {id} of kind '{kind}', expected {}",
            allowed.join(" or ")
        ))),
    }
}

fn validate_supplied_id(
    id: i64,
    context: &str,
    supplied: &mut HashSet<i64>,
) -> Result<(), ServiceError> {
    if id < 0 {
        return Err(ServiceError::UnprocessableEntity(format!(
            "{context} has invalid node id {id}"
        )));
    }
    if id != 0 && !supplied.insert(id) {
        return Err(ServiceError::UnprocessableEntity(format!(
            "{context} reuses node id {id} within the plan"
        )));
    }
    Ok(())
}

fn assert_acyclic_parents(tasks: &[PlanTaskDraft]) -> Result<(), ServiceError> {
    let parents: HashMap<&str, &str> = tasks
        .iter()
        .filter_map(|task| {
            task.parent
                .as_deref()
                .map(|parent| (task.key.as_str(), parent))
        })
        .collect();
    let mut done: HashSet<&str> = HashSet::new();
    let mut visiting: HashSet<&str> = HashSet::new();
    for task in tasks {
        let mut current = task.key.as_str();
        while !done.contains(current) {
            if !visiting.insert(current) {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "plan part_of relationships contain a cycle at '{}'",
                    task.key
                )));
            }
            match parents.get(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        done.extend(visiting.drain());
    }
    Ok(())
}

fn assert_acyclic_dependencies(tasks: &[PlanTaskDraft]) -> Result<(), ServiceError> {
    let dependencies: HashMap<&str, Vec<&str>> = tasks
        .iter()
        .map(|task| {
            (
                task.key.as_str(),
                task.depends_on.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let mut done = HashSet::new();
    for task in tasks {
        let mut visiting = HashSet::new();
        visit_dependencies(task.key.as_str(), &dependencies, &mut visiting, &mut done)?;
    }
    Ok(())
}

fn visit_dependencies<'a>(
    key: &'a str,
    dependencies: &HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
    done: &mut HashSet<&'a str>,
) -> Result<(), ServiceError> {
    if done.contains(key) {
        return Ok(());
    }
    if !visiting.insert(key) {
        return Err(ServiceError::UnprocessableEntity(format!(
            "plan depends_on relationships contain a cycle at '{key}'"
        )));
    }
    if let Some(children) = dependencies.get(key) {
        for child in children {
            visit_dependencies(child, dependencies, visiting, done)?;
        }
    }
    visiting.remove(key);
    done.insert(key);
    Ok(())
}

const MAX_TAG_LENGTH: usize = 128;
const MAX_TAGS: usize = 64;

fn normalize_property_declarations(
    properties: &[PackProperty],
) -> Result<Vec<PackProperty>, ServiceError> {
    let mut normalized: Vec<PackProperty> = Vec::new();
    for property in properties {
        let key = property.key.trim().to_string();
        if key.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "property key must not be empty".to_string(),
            ));
        }
        if key.chars().count() > MAX_VOCABULARY_NAME_LENGTH {
            return Err(ServiceError::UnprocessableEntity(format!(
                "property key '{key}' exceeds {MAX_VOCABULARY_NAME_LENGTH} characters"
            )));
        }
        if key.chars().any(char::is_control) {
            return Err(ServiceError::UnprocessableEntity(format!(
                "property key '{key}' must not contain control characters"
            )));
        }
        if key.contains('"') {
            return Err(ServiceError::UnprocessableEntity(format!(
                "property key '{key}' must not contain a double quote"
            )));
        }
        let value_type = PropertyType::parse(property.value_type.as_deref().unwrap_or("any"))
            .ok_or_else(|| {
                ServiceError::UnprocessableEntity(format!(
                    "property '{key}' has an unknown type '{}'",
                    property.value_type.as_deref().unwrap_or("")
                ))
            })?;
        if normalized.iter().any(|existing| existing.key == key) {
            return Err(ServiceError::UnprocessableEntity(format!(
                "property '{key}' appears more than once"
            )));
        }
        normalized.push(PackProperty {
            key,
            value_type: Some(value_type.as_str().to_string()),
            required: property.required,
            indexed: property.indexed,
        });
    }
    normalized.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(normalized)
}

fn encode_properties(
    properties: Option<&serde_json::Value>,
) -> Result<Option<String>, ServiceError> {
    match properties {
        None => Ok(None),
        Some(value) if value.is_object() => {
            serde_json::to_string(value).map(Some).map_err(|error| {
                ServiceError::UnprocessableEntity(format!(
                    "properties must be a JSON object: {error}"
                ))
            })
        }
        Some(_) => Err(ServiceError::UnprocessableEntity(
            "properties must be a JSON object".to_string(),
        )),
    }
}

fn parse_properties(properties: &str) -> serde_json::Value {
    serde_json::from_str(properties).unwrap_or_else(|_| serde_json::json!({}))
}

const MAX_NODE_NAME_LENGTH: usize = 512;

fn node_name(name: &str) -> Result<String, ServiceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ServiceError::UnprocessableEntity(
            "node name must not be empty".to_string(),
        ));
    }
    if name.chars().count() > MAX_NODE_NAME_LENGTH {
        return Err(ServiceError::UnprocessableEntity(format!(
            "node name exceeds {MAX_NODE_NAME_LENGTH} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ServiceError::UnprocessableEntity(
            "node name must not contain control characters".to_string(),
        ));
    }
    Ok(name.to_string())
}

const MAX_VOCABULARY_NAME_LENGTH: usize = 128;

fn vocabulary_name(name: &str, label: &str) -> Result<String, ServiceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ServiceError::UnprocessableEntity(format!(
            "{label} name must not be empty"
        )));
    }
    if name.chars().count() > MAX_VOCABULARY_NAME_LENGTH {
        return Err(ServiceError::UnprocessableEntity(format!(
            "{label} name exceeds {MAX_VOCABULARY_NAME_LENGTH} characters"
        )));
    }
    if name.contains(',') {
        return Err(ServiceError::UnprocessableEntity(format!(
            "{label} name must not contain ','"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ServiceError::UnprocessableEntity(format!(
            "{label} name must not contain control characters"
        )));
    }
    Ok(name.to_string())
}

fn normalize_kind_list(kinds: &[String]) -> Result<Vec<String>, ServiceError> {
    let mut normalized: Vec<String> = Vec::new();
    for kind in kinds {
        if kind.trim().is_empty() {
            continue;
        }
        let kind = vocabulary_name(kind, "node kind")?;
        if !normalized.contains(&kind) {
            normalized.push(kind);
        }
    }
    Ok(normalized)
}

fn reject_duplicate_names<T, F>(items: &[T], label: &str, name: F) -> Result<(), ServiceError>
where
    F: Fn(&T) -> &str,
{
    let mut seen: HashSet<&str> = HashSet::new();
    for item in items {
        let name = name(item);
        if !seen.insert(name) {
            return Err(ServiceError::UnprocessableEntity(format!(
                "{label} '{name}' appears more than once in the pack"
            )));
        }
    }
    Ok(())
}

fn normalize_import_ref(reference: &ImportRef) -> Result<ImportRef, ServiceError> {
    match reference {
        ImportRef::Id(id) => Ok(ImportRef::Id(*id)),
        ImportRef::Ref(reference) => {
            let reference = reference.trim();
            if reference.is_empty() {
                return Err(ServiceError::UnprocessableEntity(
                    "ref must not be empty".to_string(),
                ));
            }
            Ok(ImportRef::Ref(reference.to_string()))
        }
        ImportRef::NaturalKey { kind, name } => {
            let kind = kind.trim();
            let name = name.trim();
            if kind.is_empty() || name.is_empty() {
                return Err(ServiceError::UnprocessableEntity(
                    "a natural-key reference requires both kind and name".to_string(),
                ));
            }
            Ok(ImportRef::NaturalKey {
                kind: kind.to_string(),
                name: name.to_string(),
            })
        }
    }
}

fn normalize_tags(tags: &[String]) -> Result<Vec<String>, ServiceError> {
    let mut normalized: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            return Err(ServiceError::UnprocessableEntity(
                "tags must not be blank".to_string(),
            ));
        }
        if tag.chars().count() > MAX_TAG_LENGTH {
            return Err(ServiceError::UnprocessableEntity(format!(
                "tag exceeds {MAX_TAG_LENGTH} characters"
            )));
        }
        if seen.insert(tag.clone()) {
            if normalized.len() == MAX_TAGS {
                return Err(ServiceError::UnprocessableEntity(format!(
                    "at most {MAX_TAGS} tags are allowed"
                )));
            }
            normalized.push(tag);
        }
    }
    normalized.sort();
    Ok(normalized)
}

fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.to_lowercase()))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

#[cfg(test)]
mod tests {
    use crate::{
        database::{create_sqlite_pool, migrate},
        model::{
            ImportConflict, ImportRef, PlanAnchorDraft, PlanGoalDraft, RelationDirection,
            SearchScope,
        },
        repository::graph::SqliteGraphRepository,
    };
    use sqlx::SqlitePool;

    use super::*;

    async fn service() -> GraphService {
        let config = crate::config::DatabaseConfig {
            path: ":memory:".to_string(),
            max_connections: 1,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");
        let repository: Arc<dyn GraphRepository> = Arc::new(SqliteGraphRepository::new(pool));
        crate::test_support::seed_vocabulary(&repository).await;
        GraphService::new(repository, crate::config::PagingConfig::default())
    }

    async fn service_with_pool() -> (GraphService, SqlitePool) {
        let config = crate::config::DatabaseConfig {
            path: ":memory:".to_string(),
            max_connections: 1,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");
        let repository: Arc<dyn GraphRepository> =
            Arc::new(SqliteGraphRepository::new(pool.clone()));
        crate::test_support::seed_vocabulary(&repository).await;
        (
            GraphService::new(repository, crate::config::PagingConfig::default()),
            pool,
        )
    }

    fn node_kind(name: &str) -> NodeKind {
        NodeKind {
            id: 0,
            version: 0,
            name: name.to_string(),
            description: String::new(),
            unique_name: true,
            strict_properties: false,
            updated_at: String::new(),
        }
    }

    fn edge_type(name: &str, description: &str) -> EdgeType {
        EdgeType {
            id: 0,
            name: name.to_string(),
            description: description.to_string(),
            resolves: false,
            single_outgoing: false,
            acyclic: false,
            source_kinds: None,
            destination_kinds: None,
            updated_at: String::new(),
        }
    }

    fn node(kind_id: i64, name: &str, content: &str) -> Node {
        Node {
            id: 0,
            kind_id,
            name: name.to_string(),
            description: String::new(),
            content: content.to_string(),
            updated_at: String::new(),
        }
    }

    fn described_node(kind_id: i64, name: &str, description: &str) -> Node {
        Node {
            id: 0,
            kind_id,
            name: name.to_string(),
            description: description.to_string(),
            content: String::new(),
            updated_at: String::new(),
        }
    }

    async fn seed_person(service: &GraphService) -> i64 {
        service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn creates_and_lists_node_kinds() {
        let service = service().await;
        let created = service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert!(service.list_node_kinds().await.unwrap().contains(&created));
    }

    #[tokio::test]
    async fn rejects_duplicate_node_kind() {
        let service = service().await;
        service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap();
        assert!(matches!(
            service.create_node_kind(&node_kind("person")).await,
            Err(ServiceError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn creates_and_lists_edge_types() {
        let service = service().await;
        let created = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert!(service.list_edge_types().await.unwrap().contains(&created));
    }

    #[tokio::test]
    async fn assigns_id_on_insert() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", ""))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert_eq!(service.get_node(created.id).await.unwrap(), created);
    }

    #[tokio::test]
    async fn rejects_node_with_unknown_kind() {
        let service = service().await;
        assert!(matches!(
            service.upsert_node(&node(404, "Ada", "")).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn keeps_id_on_conflict() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", "a"))
            .await
            .unwrap();
        let mut changed = created.clone();
        changed.content = "b".to_string();
        let updated = service.upsert_node(&changed).await.unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.content, "b");
        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn preserves_updated_at_when_node_is_unchanged() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", "a"))
            .await
            .unwrap();
        assert!(!created.updated_at.is_empty());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let updated = service.upsert_node(&created).await.unwrap();
        assert_eq!(updated.updated_at, created.updated_at);
    }

    #[tokio::test]
    async fn bumps_updated_at_when_node_changes() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", "a"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let mut changed = created.clone();
        changed.content = "b".to_string();
        let updated = service.upsert_node(&changed).await.unwrap();
        assert_ne!(updated.updated_at, created.updated_at);
    }

    #[tokio::test]
    async fn deletes_node_and_fts_row() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&described_node(kind_id, "Ada", "ada lovelace"))
            .await
            .unwrap();
        assert_eq!(
            service
                .search(Some("ada lovelace"), &[], &Default::default(), None, None)
                .await
                .unwrap()
                .hits
                .len(),
            1
        );
        service.delete_node(created.id).await.unwrap();
        assert_eq!(
            service.delete_node(created.id).await.unwrap_err(),
            ServiceError::EntityNotFound
        );
        assert!(
            service
                .search(Some("ada lovelace"), &[], &Default::default(), None, None)
                .await
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[tokio::test]
    async fn traverses_directed_edges() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&node(kind_id, "Ada", ""))
            .await
            .unwrap();
        let bob = service
            .upsert_node(&node(kind_id, "Bob", ""))
            .await
            .unwrap();
        let knows = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        let mut edge = Edge {
            source: ada.id,
            destination: bob.id,
            edge_type_id: knows.id,
            created_at: String::new(),
        };
        service.add_edge(&edge).await.unwrap();
        service.add_edge(&edge).await.unwrap();
        let neighbors = service
            .neighbors(ada.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node, bob);
        assert_eq!(neighbors[0].direction, RelationDirection::Outgoing);
        edge.created_at = neighbors[0].edge.created_at.clone();
        assert!(!edge.created_at.is_empty());
        assert_eq!(neighbors[0].edge, edge);
        assert!(
            service
                .neighbors(bob.id, NeighborDirection::Outgoing, None)
                .await
                .unwrap()
                .is_empty()
        );
        service.remove_edge(&edge).await.unwrap();
        assert_eq!(
            service.remove_edge(&edge).await.unwrap_err(),
            ServiceError::EntityNotFound
        );
        assert!(
            service
                .neighbors(ada.id, NeighborDirection::Outgoing, None)
                .await
                .unwrap()
                .is_empty()
        );
    }

    async fn seeded_kind(service: &GraphService, name: &str) -> i64 {
        service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == name)
            .expect("seeded node kind")
            .id
    }

    async fn seeded_edge_type(service: &GraphService, name: &str) -> i64 {
        service
            .list_edge_types()
            .await
            .unwrap()
            .into_iter()
            .find(|edge_type| edge_type.name == name)
            .expect("seeded edge type")
            .id
    }

    fn plan_task(key: &str, parent: Option<&str>, depends_on: &[&str]) -> PlanTaskDraft {
        PlanTaskDraft {
            id: 0,
            key: key.to_string(),
            name: key.to_string(),
            description: String::new(),
            content: String::new(),
            parent: parent.map(str::to_string),
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            affects: vec![],
            properties: None,
            tags: None,
        }
    }

    fn plan_draft(repo: i64, tasks: Vec<PlanTaskDraft>) -> PlanDraft {
        PlanDraft {
            goal: Some(PlanGoalDraft {
                id: 0,
                name: "ship feature".to_string(),
                description: String::new(),
                content: String::new(),
                properties: None,
                tags: None,
            }),
            goal_id: None,
            anchor: Some(PlanAnchorDraft {
                node_id: repo,
                edge_type: "belongs_to".to_string(),
            }),
            tasks,
        }
    }

    fn rejection(error: ServiceError) -> String {
        match error {
            ServiceError::UnprocessableEntity(message) => message,
            other => panic!("expected an unprocessable entity, got {other:?}"),
        }
    }

    async fn seeded_repo(service: &GraphService) -> i64 {
        let repo_kind = seeded_kind(service, "repo").await;
        service
            .upsert_node(&node(repo_kind, "acme/widget", ""))
            .await
            .unwrap()
            .id
    }

    async fn anchored_goal(service: &GraphService, repo: i64) -> i64 {
        let goal_kind = seeded_kind(service, "goal").await;
        let belongs_to = seeded_edge_type(service, "belongs_to").await;
        let goal = service
            .upsert_node(&node(goal_kind, "ship widget", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: goal.id,
                destination: repo,
                edge_type_id: belongs_to,
                created_at: String::new(),
            })
            .await
            .unwrap();
        goal.id
    }

    fn new_task(name: &str) -> TaskDraft {
        TaskDraft {
            id: 0,
            name: name.to_string(),
            description: String::new(),
            content: String::new(),
            parent: None,
            depends_on: vec![],
            affects: vec![],
            properties: None,
            tags: None,
        }
    }

    #[tokio::test]
    async fn creates_task_with_wiring() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let task_kind = seeded_kind(&service, "task").await;
        let file_kind = seeded_kind(&service, "file").await;
        let part_of = seeded_edge_type(&service, "part_of").await;
        let depends_on = seeded_edge_type(&service, "depends_on").await;
        let affects = seeded_edge_type(&service, "affects").await;

        let prerequisite = service
            .upsert_node(&node(task_kind, "prepare", ""))
            .await
            .unwrap();
        let target = service
            .upsert_node(&node(file_kind, "src/lib.rs", ""))
            .await
            .unwrap();

        let mut draft = new_task("implement");
        draft.description = "write the code".to_string();
        draft.parent = Some(goal);
        draft.depends_on = vec![prerequisite.id];
        draft.affects = vec![target.id];
        let created = service.create_task(&draft).await.unwrap();

        assert_eq!(created.kind_id, task_kind);
        let neighbors = service
            .neighbors(created.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        let wired: HashSet<(i64, i64)> = neighbors
            .iter()
            .map(|neighbor| (neighbor.edge.edge_type_id, neighbor.node.id))
            .collect();
        assert_eq!(
            wired,
            HashSet::from([
                (part_of, goal),
                (depends_on, prerequisite.id),
                (affects, target.id),
            ])
        );
    }

    #[tokio::test]
    async fn creates_plan_tree_with_dependencies() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let file_kind = seeded_kind(&service, "file").await;
        let target = service
            .upsert_node(&node(file_kind, "src/lib.rs", ""))
            .await
            .unwrap();

        let mut build = plan_task("build", Some("design"), &["design"]);
        build.affects = vec![target.id];
        let plan = service
            .create_plan(&plan_draft(
                repo,
                vec![plan_task("design", None, &[]), build],
            ))
            .await
            .unwrap();

        let goal = plan.goal.as_ref().expect("goal created");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].key, "design");
        assert_eq!(plan.tasks[1].key, "build");

        let design_neighbors = service
            .neighbors(plan.tasks[0].node.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(design_neighbors.len(), 1);
        assert_eq!(design_neighbors[0].node.id, goal.id);

        let build_neighbors = service
            .neighbors(plan.tasks[1].node.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        let destinations: HashSet<i64> = build_neighbors
            .iter()
            .map(|neighbor| neighbor.node.id)
            .collect();
        assert_eq!(
            destinations,
            HashSet::from([plan.tasks[0].node.id, target.id])
        );
    }

    #[tokio::test]
    async fn attaches_plan_to_existing_goal() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let plan = service
            .create_plan(&PlanDraft {
                goal: None,
                goal_id: Some(goal),
                anchor: None,
                tasks: vec![plan_task("t", None, &[])],
            })
            .await
            .unwrap();

        assert!(plan.goal.is_none());
        let neighbors = service
            .neighbors(plan.tasks[0].node.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node.id, goal);
    }

    #[tokio::test]
    async fn rejects_invalid_plans_without_writing() {
        let service = service().await;
        let repo = seeded_repo(&service).await;

        let duplicate = plan_draft(
            repo,
            vec![plan_task("a", None, &[]), plan_task("a", None, &[])],
        );
        assert!(matches!(
            service.create_plan(&duplicate).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let unknown_parent = plan_draft(repo, vec![plan_task("a", Some("missing"), &[])]);
        assert!(matches!(
            service.create_plan(&unknown_parent).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let unknown_dependency = plan_draft(repo, vec![plan_task("a", None, &["missing"])]);
        assert!(matches!(
            service.create_plan(&unknown_dependency).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let self_dependency = plan_draft(repo, vec![plan_task("a", None, &["a"])]);
        assert!(matches!(
            service.create_plan(&self_dependency).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let parent_cycle = plan_draft(
            repo,
            vec![
                plan_task("a", Some("b"), &[]),
                plan_task("b", Some("a"), &[]),
            ],
        );
        assert!(matches!(
            service.create_plan(&parent_cycle).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let dependency_cycle = plan_draft(
            repo,
            vec![plan_task("a", None, &["b"]), plan_task("b", None, &["a"])],
        );
        assert!(matches!(
            service.create_plan(&dependency_cycle).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let mut both_roots = plan_draft(repo, vec![]);
        both_roots.goal_id = Some(repo);
        assert!(matches!(
            service.create_plan(&both_roots).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let no_root = PlanDraft {
            goal: None,
            goal_id: None,
            anchor: None,
            tasks: vec![],
        };
        assert!(matches!(
            service.create_plan(&no_root).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn rejects_plan_with_unknown_affects_target() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let mut broken = plan_task("t", None, &[]);
        broken.affects = vec![9999];
        let draft = plan_draft(repo, vec![broken]);

        assert!(matches!(
            service.create_plan(&draft).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn rejects_self_referencing_task_id_and_rolls_back() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let mut seed = new_task("existing");
        seed.parent = Some(goal);
        let existing = service.create_task(&seed).await.unwrap();

        let mut draft = new_task("self");
        draft.id = existing.id;
        draft.parent = Some(existing.id);

        assert!(matches!(
            service.create_task(&draft).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert_eq!(
            service.get_node(existing.id).await.unwrap().name,
            "existing"
        );
        let neighbors = service
            .neighbors(existing.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node.id, goal);
        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn rejects_task_wiring_to_wrong_kinds() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let file_kind = seeded_kind(&service, "file").await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let file = service
            .upsert_node(&node(file_kind, "src/lib.rs", ""))
            .await
            .unwrap();
        let other_goal = service
            .upsert_node(&node(goal_kind, "other", ""))
            .await
            .unwrap();

        let mut parent = new_task("a");
        parent.parent = Some(file.id);
        assert!(matches!(
            service.create_task(&parent).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let mut dependency = new_task("b");
        dependency.parent = Some(goal);
        dependency.depends_on = vec![file.id];
        assert!(matches!(
            service.create_task(&dependency).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let mut affects = new_task("c");
        affects.parent = Some(goal);
        affects.affects = vec![other_goal.id];
        assert!(matches!(
            service.create_task(&affects).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn rejects_plan_targets_of_wrong_kind() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let task_kind = seeded_kind(&service, "task").await;
        let file_kind = seeded_kind(&service, "file").await;
        let existing_task = service
            .upsert_node(&node(task_kind, "existing", ""))
            .await
            .unwrap();
        let file = service
            .upsert_node(&node(file_kind, "src/lib.rs", ""))
            .await
            .unwrap();

        let mut affects = plan_task("a", None, &[]);
        affects.affects = vec![existing_task.id];
        assert!(matches!(
            service.create_plan(&plan_draft(repo, vec![affects])).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let draft = PlanDraft {
            goal: None,
            goal_id: Some(file.id),
            anchor: None,
            tasks: vec![plan_task("a", None, &[])],
        };
        assert!(matches!(
            service.create_plan(&draft).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn upserts_task_with_supplied_id() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let mut first = new_task("implement");
        first.id = 100;
        first.parent = Some(goal);
        service.create_task(&first).await.unwrap();

        let mut second = new_task("implement");
        second.id = 100;
        second.parent = Some(goal);
        second.description = "changed".to_string();
        service.create_task(&second).await.unwrap();

        let nodes = service.list_nodes(&NodeFilter::default()).await.unwrap();
        assert_eq!(nodes.len(), 3);
        let created = nodes
            .iter()
            .find(|node| node.id == 100)
            .expect("upserted task");
        assert_eq!(created.description, "changed");
    }

    #[tokio::test]
    async fn upserts_plan_with_supplied_ids() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let mut task = plan_task("a", None, &[]);
        task.id = 101;
        let draft = PlanDraft {
            goal: Some(PlanGoalDraft {
                id: 100,
                name: "ship".to_string(),
                description: String::new(),
                content: String::new(),
                properties: None,
                tags: None,
            }),
            goal_id: None,
            anchor: Some(PlanAnchorDraft {
                node_id: repo,
                edge_type: "belongs_to".to_string(),
            }),
            tasks: vec![task],
        };

        service.create_plan(&draft).await.unwrap();
        service.create_plan(&draft).await.unwrap();

        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn rejects_invalid_supplied_ids() {
        let service = service().await;
        let repo = seeded_repo(&service).await;

        let mut negative = new_task("a");
        negative.id = -1;
        assert!(matches!(
            service.create_task(&negative).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let mut first = plan_task("a", None, &[]);
        first.id = 100;
        let mut second = plan_task("b", None, &[]);
        second.id = 100;
        assert!(matches!(
            service
                .create_plan(&plan_draft(repo, vec![first, second]))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn checks_large_part_of_chains_in_linear_time() {
        let tasks: Vec<PlanTaskDraft> = (0..20_000)
            .map(|index| {
                let mut task = plan_task(&format!("k{index}"), None, &[]);
                if index > 0 {
                    task.parent = Some(format!("k{}", index - 1));
                }
                task
            })
            .collect();

        let started = std::time::Instant::now();
        assert!(assert_acyclic_parents(&tasks).is_ok());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn traverses_incoming_and_both_neighbors() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&node(kind_id, "Ada", ""))
            .await
            .unwrap();
        let bob = service
            .upsert_node(&node(kind_id, "Bob", ""))
            .await
            .unwrap();
        let knows = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: ada.id,
                destination: bob.id,
                edge_type_id: knows.id,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let incoming = service
            .neighbors(bob.id, NeighborDirection::Incoming, None)
            .await
            .unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].direction, RelationDirection::Ingoing);
        assert_eq!(incoming[0].node, ada);

        let both = service
            .neighbors(ada.id, NeighborDirection::Both, None)
            .await
            .unwrap();
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].direction, RelationDirection::Outgoing);

        let filtered = service
            .neighbors(bob.id, NeighborDirection::Incoming, Some("knows"))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        let unmatched = service
            .neighbors(bob.id, NeighborDirection::Incoming, Some("contains"))
            .await
            .unwrap();
        assert!(unmatched.is_empty());
    }

    #[tokio::test]
    async fn rejects_unknown_edge_type_filter() {
        let service = service().await;
        assert!(matches!(
            service
                .neighbors(1, NeighborDirection::Outgoing, Some("nope"))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn updates_task_description_without_touching_wiring() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let mut draft = new_task("implement");
        draft.parent = Some(goal);
        let created = service.create_task(&draft).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let updated = service
            .update_task(
                created.id,
                &TaskPatch {
                    name: None,
                    description: Some("recalibrated widget".to_string()),
                    content: None,
                    properties: None,
                    tags: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "implement");
        assert_eq!(updated.description, "recalibrated widget");
        assert_ne!(updated.updated_at, created.updated_at);

        let neighbors = service
            .neighbors(created.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node.id, goal);

        let page = service
            .search(
                Some("recalibrated widget"),
                &[],
                &Default::default(),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(page.hits[0].node.id, created.id);
    }

    #[tokio::test]
    async fn rejects_empty_or_non_task_patch() {
        let service = service().await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let goal = service
            .upsert_node(&node(goal_kind, "g", ""))
            .await
            .unwrap();

        assert!(matches!(
            service
                .update_task(
                    goal.id,
                    &TaskPatch {
                        name: None,
                        description: None,
                        content: None,
                        properties: None,
                        tags: None,
                    },
                )
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .update_task(
                    goal.id,
                    &TaskPatch {
                        name: None,
                        description: Some("x".to_string()),
                        content: None,
                        properties: None,
                        tags: None,
                    },
                )
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert_eq!(
            service
                .update_task(
                    404,
                    &TaskPatch {
                        name: None,
                        description: Some("x".to_string()),
                        content: None,
                        properties: None,
                        tags: None,
                    },
                )
                .await
                .unwrap_err(),
            ServiceError::EntityNotFound
        );
    }

    #[tokio::test]
    async fn rejects_resolving_edge_to_non_resolvable_kind() {
        let service = service().await;
        let task_kind = seeded_kind(&service, "task").await;
        let change_kind = seeded_kind(&service, "change").await;
        let file_kind = seeded_kind(&service, "file").await;
        let implements = seeded_edge_type(&service, "implements").await;

        let change = service
            .upsert_node(&node(change_kind, "commit", ""))
            .await
            .unwrap();
        let task = service
            .upsert_node(&node(task_kind, "work", ""))
            .await
            .unwrap();
        let file = service
            .upsert_node(&node(file_kind, "src/x.rs", ""))
            .await
            .unwrap();

        service
            .add_edge(&Edge {
                source: change.id,
                destination: task.id,
                edge_type_id: implements,
                created_at: String::new(),
            })
            .await
            .unwrap();
        assert!(matches!(
            service
                .add_edge(&Edge {
                    source: change.id,
                    destination: file.id,
                    edge_type_id: implements,
                    created_at: String::new(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(
            service
                .neighbors(file.id, NeighborDirection::Incoming, Some("implements"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    async fn repo_goal_task_graph(service: &GraphService) -> (i64, i64, i64, i64, i64) {
        let repo_kind = seeded_kind(service, "repo").await;
        let goal_kind = seeded_kind(service, "goal").await;
        let task_kind = seeded_kind(service, "task").await;
        let belongs_to = seeded_edge_type(service, "belongs_to").await;
        let part_of = seeded_edge_type(service, "part_of").await;

        let repo = service
            .upsert_node(&node(repo_kind, "acme/widget", ""))
            .await
            .unwrap();
        let goal = service
            .upsert_node(&node(goal_kind, "ship widget", ""))
            .await
            .unwrap();
        let other_goal = service
            .upsert_node(&node(goal_kind, "ship gadget", ""))
            .await
            .unwrap();
        let task = service
            .upsert_node(&node(task_kind, "build widget", ""))
            .await
            .unwrap();
        let other_task = service
            .upsert_node(&node(task_kind, "build gadget", ""))
            .await
            .unwrap();

        service
            .add_edge(&Edge {
                source: goal.id,
                destination: repo.id,
                edge_type_id: belongs_to,
                created_at: String::new(),
            })
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: task.id,
                destination: goal.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: other_task.id,
                destination: other_goal.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap();

        (repo.id, goal.id, other_goal.id, task.id, other_task.id)
    }

    #[tokio::test]
    async fn enforces_declared_edge_kind_constraints() {
        let service = service().await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let file_kind = seeded_kind(&service, "file").await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let belongs_to = seeded_edge_type(&service, "belongs_to").await;
        let contains = seeded_edge_type(&service, "contains").await;

        let repo = service
            .upsert_node(&node(repo_kind, "acme/widget", ""))
            .await
            .unwrap();
        let file = service
            .upsert_node(&node(file_kind, "src/lib.rs", ""))
            .await
            .unwrap();
        let goal = service
            .upsert_node(&node(goal_kind, "ship", ""))
            .await
            .unwrap();

        service
            .add_edge(&Edge {
                source: goal.id,
                destination: repo.id,
                edge_type_id: belongs_to,
                created_at: String::new(),
            })
            .await
            .unwrap();
        assert!(matches!(
            service
                .add_edge(&Edge {
                    source: repo.id,
                    destination: goal.id,
                    edge_type_id: belongs_to,
                    created_at: String::new(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .add_edge(&Edge {
                    source: goal.id,
                    destination: file.id,
                    edge_type_id: belongs_to,
                    created_at: String::new(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        service
            .add_edge(&Edge {
                source: repo.id,
                destination: file.id,
                edge_type_id: contains,
                created_at: String::new(),
            })
            .await
            .unwrap();
        assert!(matches!(
            service
                .add_edge(&Edge {
                    source: file.id,
                    destination: repo.id,
                    edge_type_id: contains,
                    created_at: String::new(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn filters_nodes_by_anchor_and_part_of_descendants() {
        let service = service().await;
        let (repo, goal, _other_goal, task, other_task) = repo_goal_task_graph(&service).await;

        let scoped = service
            .list_nodes(&NodeFilter {
                properties: Default::default(),
                kind: Some("task".to_string()),
                scope: SearchScope::All,
                reach: ReachFilter {
                    within: Some(repo),
                    via: Some("belongs_to".to_string()),
                    descend: None,
                },
                name: None,
                name_contains: None,
            })
            .await
            .unwrap();
        assert_eq!(
            scoped.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![task]
        );
        assert!(!scoped.iter().any(|node| node.id == other_task));

        let anchored = service
            .list_nodes(&NodeFilter {
                properties: Default::default(),
                kind: Some("goal".to_string()),
                scope: SearchScope::All,
                reach: ReachFilter {
                    within: Some(repo),
                    via: Some("belongs_to".to_string()),
                    descend: Some("none".to_string()),
                },
                name: None,
                name_contains: None,
            })
            .await
            .unwrap();
        assert_eq!(
            anchored.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![goal]
        );

        let unfiltered = service.list_nodes(&NodeFilter::default()).await.unwrap();
        assert_eq!(unfiltered.len(), 5);
    }

    #[tokio::test]
    async fn rejects_invalid_node_filters() {
        let service = service().await;
        let (repo, ..) = repo_goal_task_graph(&service).await;

        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    properties: Default::default(),
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: None,
                        via: Some("belongs_to".to_string()),
                        descend: None,
                    },
                    name: None,
                    name_contains: None,
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    properties: Default::default(),
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: Some(repo),
                        via: Some("nope".to_string()),
                        descend: None,
                    },
                    name: None,
                    name_contains: None,
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    properties: Default::default(),
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: Some(repo),
                        via: Some(DESCEND_NONE.to_string()),
                        descend: None,
                    },
                    name: None,
                    name_contains: None,
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    properties: Default::default(),
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: Some(9999),
                        via: None,
                        descend: None,
                    },
                    name: None,
                    name_contains: None,
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    properties: Default::default(),
                    kind: Some("nope".to_string()),
                    scope: SearchScope::All,
                    reach: ReachFilter::default(),
                    name: None,
                    name_contains: None,
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn filters_nodes_by_resolution_scope() {
        let service = service().await;
        let (repo, _goal, _other_goal, task, _other_task) = repo_goal_task_graph(&service).await;
        let change_kind = seeded_kind(&service, "change").await;
        let implements = seeded_edge_type(&service, "implements").await;
        let change = service
            .upsert_node(&node(change_kind, "commit", ""))
            .await
            .unwrap();

        let filter = |scope| NodeFilter {
            properties: Default::default(),
            kind: Some("task".to_string()),
            scope,
            reach: ReachFilter {
                within: Some(repo),
                via: Some("belongs_to".to_string()),
                descend: None,
            },
            name: None,
            name_contains: None,
        };
        assert_eq!(
            service
                .list_nodes(&filter(SearchScope::Active))
                .await
                .unwrap()
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![task]
        );

        service
            .add_edge(&Edge {
                source: change.id,
                destination: task,
                edge_type_id: implements,
                created_at: String::new(),
            })
            .await
            .unwrap();

        assert!(
            service
                .list_nodes(&filter(SearchScope::Active))
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            service
                .list_nodes(&filter(SearchScope::Resolved))
                .await
                .unwrap()
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![task]
        );
    }

    #[tokio::test]
    async fn plan_anchor_wires_the_goal_to_the_anchor_node() {
        let service = service().await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let belongs_to = seeded_edge_type(&service, "belongs_to").await;
        let repo = service
            .upsert_node(&node(repo_kind, "acme/widget", ""))
            .await
            .unwrap();

        let draft = |anchor: PlanAnchorDraft, goal: &str| PlanDraft {
            goal: Some(PlanGoalDraft {
                id: 0,
                name: goal.to_string(),
                description: String::new(),
                content: String::new(),
                properties: None,
                tags: None,
            }),
            goal_id: None,
            anchor: Some(anchor),
            tasks: vec![plan_task("a", None, &[])],
        };

        let plan = service
            .create_plan(&draft(
                PlanAnchorDraft {
                    node_id: repo.id,
                    edge_type: "belongs_to".to_string(),
                },
                "ship widget",
            ))
            .await
            .unwrap();
        let goal = plan.goal.as_ref().expect("goal created");
        let neighbors = service
            .neighbors(goal.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert!(neighbors.iter().any(
            |neighbor| neighbor.edge.edge_type_id == belongs_to && neighbor.node.id == repo.id
        ));

        assert!(matches!(
            service
                .create_plan(&draft(
                    PlanAnchorDraft {
                        node_id: repo.id,
                        edge_type: "contains".to_string(),
                    },
                    "invalid contract",
                ))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .create_plan(&draft(
                    PlanAnchorDraft {
                        node_id: 9999,
                        edge_type: "belongs_to".to_string(),
                    },
                    "missing anchor",
                ))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .create_plan(&draft(
                    PlanAnchorDraft {
                        node_id: repo.id,
                        edge_type: "nope".to_string(),
                    },
                    "unknown edge type",
                ))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn requires_a_parent_for_standalone_tasks() {
        let service = service().await;
        let error = service.create_task(&new_task("orphan")).await.unwrap_err();
        assert!(rejection(error).contains("requires a parent"));
        assert!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rejects_unanchored_task_parents() {
        let service = service().await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let goal = service
            .upsert_node(&node(goal_kind, "loose goal", ""))
            .await
            .unwrap();

        let mut draft = new_task("a");
        draft.parent = Some(goal.id);
        let error = service.create_task(&draft).await.unwrap_err();
        assert!(rejection(error).contains("is not attached to a repo"));
        assert_eq!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn anchors_task_parents_transitively() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let mut parent = new_task("parent");
        parent.parent = Some(goal);
        let parent = service.create_task(&parent).await.unwrap();

        let mut child = new_task("child");
        child.parent = Some(parent.id);
        let child = service.create_task(&child).await.unwrap();

        let neighbors = service
            .neighbors(child.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node.id, parent.id);
    }

    #[tokio::test]
    async fn requires_an_anchor_for_new_plan_goals() {
        let service = service().await;
        let draft = PlanDraft {
            goal: Some(PlanGoalDraft {
                id: 0,
                name: "ship feature".to_string(),
                description: String::new(),
                content: String::new(),
                properties: None,
                tags: None,
            }),
            goal_id: None,
            anchor: None,
            tasks: vec![plan_task("a", None, &[])],
        };
        let error = service.create_plan(&draft).await.unwrap_err();
        assert!(rejection(error).contains("requires an anchor"));
        assert!(
            service
                .list_nodes(&NodeFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rejects_unanchored_existing_goal_without_an_anchor() {
        let service = service().await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let goal = service
            .upsert_node(&node(goal_kind, "loose goal", ""))
            .await
            .unwrap();

        let draft = PlanDraft {
            goal: None,
            goal_id: Some(goal.id),
            anchor: None,
            tasks: vec![plan_task("a", None, &[])],
        };
        let error = service.create_plan(&draft).await.unwrap_err();
        assert!(rejection(error).contains("is not attached to a repo"));

        let repo = seeded_repo(&service).await;
        let anchored = PlanDraft {
            goal: None,
            goal_id: Some(goal.id),
            anchor: Some(PlanAnchorDraft {
                node_id: repo,
                edge_type: "belongs_to".to_string(),
            }),
            tasks: vec![plan_task("a", None, &[])],
        };
        assert!(service.create_plan(&anchored).await.is_ok());
    }

    #[tokio::test]
    async fn rejects_re_anchoring_a_goal_to_a_second_repo() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let belongs_to = seeded_edge_type(&service, "belongs_to").await;
        let other_repo = service
            .upsert_node(&node(repo_kind, "acme/other", ""))
            .await
            .unwrap()
            .id;

        let draft = PlanDraft {
            goal: None,
            goal_id: Some(goal),
            anchor: Some(PlanAnchorDraft {
                node_id: other_repo,
                edge_type: "belongs_to".to_string(),
            }),
            tasks: vec![plan_task("a", None, &[])],
        };
        let error = service.create_plan(&draft).await.unwrap_err();
        assert!(rejection(error).contains("allows one outgoing edge"));

        assert!(matches!(
            service
                .add_edge(&Edge {
                    source: goal,
                    destination: other_repo,
                    edge_type_id: belongs_to,
                    created_at: String::new(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let anchored_once = service
            .list_nodes(&NodeFilter {
                properties: Default::default(),
                kind: Some("goal".to_string()),
                scope: SearchScope::All,
                reach: ReachFilter {
                    within: Some(other_repo),
                    via: Some("belongs_to".to_string()),
                    descend: Some("none".to_string()),
                },
                name: None,
                name_contains: None,
            })
            .await
            .unwrap();
        assert!(anchored_once.is_empty());

        let same_repo = PlanDraft {
            goal: None,
            goal_id: Some(goal),
            anchor: Some(PlanAnchorDraft {
                node_id: repo,
                edge_type: "belongs_to".to_string(),
            }),
            tasks: vec![plan_task("a", None, &[])],
        };
        assert!(service.create_plan(&same_repo).await.is_ok());
    }

    #[tokio::test]
    async fn trigger_rejects_part_of_cycles_and_loops() {
        let service = service().await;
        let task_kind = seeded_kind(&service, "task").await;
        let part_of = seeded_edge_type(&service, "part_of").await;
        let first = service
            .upsert_node(&node(task_kind, "first", ""))
            .await
            .unwrap();
        let second = service
            .upsert_node(&node(task_kind, "second", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: first.id,
                destination: second.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let cycle = service
            .add_edge(&Edge {
                source: second.id,
                destination: first.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap_err();
        assert!(rejection(cycle).contains("cycle"));

        let self_loop = service
            .add_edge(&Edge {
                source: first.id,
                destination: first.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap_err();
        assert!(rejection(self_loop).contains("cycle"));
    }

    #[tokio::test]
    async fn a_non_acyclic_edge_type_allows_a_cycle() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let knows = service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "knows".to_string(),
                description: "person -> person".to_string(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: Some("person".to_string()),
                destination_kinds: Some("person".to_string()),
                updated_at: String::new(),
            })
            .await
            .unwrap()
            .id;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;
        for (source, destination) in [(first.id, second.id), (second.id, first.id)] {
            service
                .add_edge(&Edge {
                    source,
                    destination,
                    edge_type_id: knows,
                    created_at: String::new(),
                })
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn pack_can_declare_an_acyclic_edge_type() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![PackEdgeType {
                    name: "supervises".to_string(),
                    description: Some("person -> person".to_string()),
                    resolves: None,
                    single_outgoing: None,
                    acyclic: Some(true),
                    source_kinds: Some(vec!["person".to_string()]),
                    destination_kinds: Some(vec!["person".to_string()]),
                }],
            })
            .await
            .unwrap();
        let supervises = seeded_edge_type(&service, "supervises").await;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;
        service
            .add_edge(&Edge {
                source: first.id,
                destination: second.id,
                edge_type_id: supervises,
                created_at: String::new(),
            })
            .await
            .unwrap();
        let cycle = service
            .add_edge(&Edge {
                source: second.id,
                destination: first.id,
                edge_type_id: supervises,
                created_at: String::new(),
            })
            .await
            .unwrap_err();
        assert!(rejection(cycle).contains("cycle"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn trigger_serializes_concurrent_single_outgoing_edges() {
        let path = temp_sqlite("memgraph-single-outgoing");
        let service = file_service(&path).await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let belongs_to = seeded_edge_type(&service, "belongs_to").await;
        let goal = service
            .create_node(&node(goal_kind, "race goal", ""), &[])
            .await
            .unwrap()
            .0;
        let mut repos = Vec::new();
        for index in 0..8 {
            repos.push(
                service
                    .create_node(&node(repo_kind, &format!("acme/{index}"), ""), &[])
                    .await
                    .unwrap()
                    .0
                    .id,
            );
        }

        let mut handles = Vec::new();
        for repo in repos {
            let service = service.clone();
            handles.push(tokio::spawn(async move {
                service
                    .add_edge(&Edge {
                        source: goal.id,
                        destination: repo,
                        edge_type_id: belongs_to,
                        created_at: String::new(),
                    })
                    .await
            }));
        }
        let mut accepted = 0;
        let mut rejected = 0;
        for handle in handles {
            match handle.await.unwrap() {
                Ok(()) => accepted += 1,
                Err(ServiceError::UnprocessableEntity(_)) => rejected += 1,
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }
        assert_eq!(accepted, 1);
        assert_eq!(rejected, 7);
        let edges = service
            .neighbors(goal.id, NeighborDirection::Outgoing, Some("belongs_to"))
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);

        drop(service);
        remove_sqlite(&path);
    }

    #[tokio::test]
    async fn creates_node_with_normalized_tags() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let (created, tags) = service
            .create_node(
                &node(kind_id, "Ada", "pioneer of computing"),
                &[
                    " Math ".to_string(),
                    "math".to_string(),
                    " LOGIC ".to_string(),
                ],
            )
            .await
            .unwrap();
        assert_eq!(tags, vec!["logic", "math"]);
        assert_eq!(created.id, service.get_node(created.id).await.unwrap().id);
        assert_eq!(
            service.node_tags(created.id).await.unwrap(),
            vec!["logic", "math"]
        );
    }

    #[tokio::test]
    async fn rejects_blank_tags_and_tag_lists_over_the_cap() {
        let service = service().await;
        let kind_id = seed_person(&service).await;

        let blank = service
            .create_node(
                &node(kind_id, "Ada", ""),
                &["math".to_string(), "   ".to_string()],
            )
            .await;
        assert!(matches!(
            blank,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("blank")
        ));

        let at_cap: Vec<String> = (0..64).map(|index| format!("tag{index}")).collect();
        service
            .create_node(&node(kind_id, "Grace", ""), &at_cap)
            .await
            .expect("64 tags are allowed");

        let over_cap: Vec<String> = (0..65).map(|index| format!("tag{index}")).collect();
        let over = service
            .create_node(&node(kind_id, "Edsger", ""), &over_cap)
            .await;
        assert!(matches!(
            over,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("at most 64 tags")
        ));

        let duplicates: Vec<String> = (0..200).map(|_| "same".to_string()).collect();
        let (_, tags) = service
            .create_node(&node(kind_id, "Barbara", ""), &duplicates)
            .await
            .expect("duplicates collapse before the cap");
        assert_eq!(tags, vec!["same"]);
    }

    #[tokio::test]
    async fn replaces_tags_and_reports_usage() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let first = service
            .create_node(&node(kind_id, "Ada", ""), &["math".to_string()])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(
                &node(kind_id, "Bob", ""),
                &["math".to_string(), "logic".to_string()],
            )
            .await
            .unwrap()
            .0;

        let replaced = service
            .replace_node_tags(first.id, &["logic".to_string()])
            .await
            .unwrap();
        assert_eq!(replaced, vec!["logic"]);

        let usage = service.list_tags().await.unwrap();
        assert_eq!(
            usage
                .iter()
                .map(|tag| (tag.tag.as_str(), tag.node_count))
                .collect::<Vec<_>>(),
            vec![("logic", 2), ("math", 1)]
        );
        assert_eq!(
            service.node_tags(second.id).await.unwrap(),
            vec!["logic", "math"]
        );
    }

    #[tokio::test]
    async fn search_covers_description_and_content() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let described = service
            .create_node(&described_node(kind_id, "Ada", "pioneer of computing"), &[])
            .await
            .unwrap()
            .0;
        let bodied = service
            .create_node(&node(kind_id, "Notes", "pioneer of computing"), &[])
            .await
            .unwrap()
            .0;

        let page = service
            .search(
                Some("pioneer computing"),
                &[],
                &Default::default(),
                None,
                None,
            )
            .await
            .unwrap();
        let ids: Vec<i64> = page.hits.iter().map(|hit| hit.node.id).collect();
        assert!(ids.contains(&described.id));
        assert!(ids.contains(&bodied.id));
        assert_eq!(page.total, 2);
        assert!(page.hits.iter().all(|hit| hit.score >= 0.0));
    }

    #[tokio::test]
    async fn search_requires_every_tag() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let both = service
            .create_node(
                &described_node(kind_id, "Both", "shared text"),
                &["alpha".to_string(), "beta".to_string()],
            )
            .await
            .unwrap()
            .0;
        service
            .create_node(
                &described_node(kind_id, "OnlyAlpha", "shared text"),
                &["alpha".to_string()],
            )
            .await
            .unwrap();

        let page = service
            .search(
                None,
                &["alpha".to_string(), "beta".to_string()],
                &Default::default(),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].node.id, both.id);
        assert_eq!(page.hits[0].tags, vec!["alpha", "beta"]);

        let combined = service
            .search(
                Some("shared"),
                &["alpha".to_string()],
                &Default::default(),
                Some(1),
                Some(0),
            )
            .await
            .unwrap();
        assert_eq!(combined.total, 2);
        assert_eq!(combined.hits.len(), 1);
        assert_eq!(combined.limit, 1);
        assert_eq!(combined.offset, 0);
    }

    #[tokio::test]
    async fn search_rejects_an_empty_query_without_tags() {
        let service = service().await;
        assert!(matches!(
            service
                .search(Some("   "), &[], &Default::default(), None, None)
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .search(None, &[], &Default::default(), None, None)
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn node_tags_on_missing_node_is_not_found() {
        let service = service().await;
        assert!(matches!(
            service.node_tags(999).await,
            Err(ServiceError::EntityNotFound)
        ));
        assert!(matches!(
            service.replace_node_tags(999, &[]).await,
            Err(ServiceError::EntityNotFound)
        ));
    }

    #[tokio::test]
    async fn search_reflects_node_updates() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let original = service
            .create_node(&described_node(kind_id, "Ada", "pioneer of computing"), &[])
            .await
            .unwrap()
            .0;
        assert_eq!(
            service
                .search(Some("pioneer"), &[], &Default::default(), None, None)
                .await
                .unwrap()
                .total,
            1
        );

        let mut changed = original.clone();
        changed.description = "mathematician and logician".to_string();
        service.upsert_node(&changed).await.unwrap();

        assert_eq!(
            service
                .search(Some("pioneer"), &[], &Default::default(), None, None)
                .await
                .unwrap()
                .total,
            0
        );
        let page = service
            .search(Some("logician"), &[], &Default::default(), None, None)
            .await
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].node.id, original.id);
    }

    fn pack_kind(name: &str, description: Option<&str>) -> PackNodeKind {
        PackNodeKind {
            name: name.to_string(),
            description: description.map(str::to_string),
            unique_name: None,
            properties: None,
            strict_properties: None,
        }
    }

    fn pack_edge_type(name: &str, source: &str, destination: &str) -> PackEdgeType {
        PackEdgeType {
            name: name.to_string(),
            description: Some(format!("{source} -> {destination}")),
            resolves: None,
            single_outgoing: None,
            acyclic: None,
            source_kinds: Some(vec![source.to_string()]),
            destination_kinds: Some(vec![destination.to_string()]),
        }
    }

    #[tokio::test]
    async fn applies_packs_idempotently_and_bumps_version_on_change() {
        let service = service().await;
        let pack = PackDraft {
            node_kinds: vec![pack_kind("widget", Some("a widget"))],
            edge_types: vec![pack_edge_type("widget_part", "widget", "widget")],
        };

        let first = service.apply_pack(&pack).await.unwrap();
        assert_eq!(
            (first.created(), first.updated(), first.unchanged()),
            (2, 0, 0)
        );
        assert_eq!(first.node_kinds[0].version, Some(1));

        let second = service.apply_pack(&pack).await.unwrap();
        assert_eq!(
            (second.created(), second.updated(), second.unchanged()),
            (0, 0, 2)
        );
        assert_eq!(second.node_kinds[0].version, Some(1));

        let mut changed = pack.clone();
        changed.node_kinds[0].description = Some("a changed widget".to_string());
        let third = service.apply_pack(&changed).await.unwrap();
        assert_eq!(
            (third.created(), third.updated(), third.unchanged()),
            (0, 1, 1)
        );
        assert_eq!(third.node_kinds[0].version, Some(2));
    }

    #[tokio::test]
    async fn trims_pack_names_before_matching() {
        let service = service().await;
        let pack = PackDraft {
            node_kinds: vec![pack_kind("  widget  ", Some("a widget"))],
            edge_types: vec![],
        };
        let first = service.apply_pack(&pack).await.unwrap();
        assert_eq!(first.node_kinds[0].name, "widget");

        let second = service
            .apply_pack(&PackDraft {
                node_kinds: vec![pack_kind("widget", Some("a widget"))],
                edge_types: vec![],
            })
            .await
            .unwrap();
        assert_eq!(second.unchanged(), 1);
    }

    #[tokio::test]
    async fn updates_edge_type_contracts_in_place() {
        let service = service().await;
        let pack = PackDraft {
            node_kinds: vec![pack_kind("widget", Some("a widget"))],
            edge_types: vec![pack_edge_type("widget_part", "widget", "widget")],
        };
        service.apply_pack(&pack).await.unwrap();

        let mut changed = pack.clone();
        changed.edge_types[0].single_outgoing = Some(true);
        let report = service.apply_pack(&changed).await.unwrap();
        assert_eq!((report.updated(), report.unchanged()), (1, 1));

        let stored = service
            .list_edge_types()
            .await
            .unwrap()
            .into_iter()
            .find(|edge_type| edge_type.name == "widget_part")
            .expect("edge type stored");
        assert!(stored.single_outgoing);
    }

    #[tokio::test]
    async fn a_partial_pack_leaves_edge_type_fields_untouched() {
        let service = service().await;
        service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "custom".to_string(),
                description: "original".to_string(),
                resolves: true,
                single_outgoing: true,
                acyclic: false,
                source_kinds: Some("task".to_string()),
                destination_kinds: Some("goal".to_string()),
                updated_at: String::new(),
            })
            .await
            .unwrap();

        let report = service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![PackEdgeType {
                    name: "custom".to_string(),
                    description: Some("changed".to_string()),
                    resolves: None,
                    single_outgoing: None,
                    acyclic: None,
                    source_kinds: None,
                    destination_kinds: None,
                }],
            })
            .await
            .unwrap();
        assert_eq!(report.updated(), 1);

        let stored = service
            .list_edge_types()
            .await
            .unwrap()
            .into_iter()
            .find(|edge_type| edge_type.name == "custom")
            .expect("edge type stored");
        assert_eq!(stored.description, "changed");
        assert!(stored.resolves);
        assert!(stored.single_outgoing);
        assert_eq!(stored.source_kinds.as_deref(), Some("task"));
        assert_eq!(stored.destination_kinds.as_deref(), Some("goal"));

        service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![PackEdgeType {
                    name: "custom".to_string(),
                    description: None,
                    resolves: None,
                    single_outgoing: None,
                    acyclic: None,
                    source_kinds: Some(vec![]),
                    destination_kinds: None,
                }],
            })
            .await
            .unwrap();
        let cleared = service
            .list_edge_types()
            .await
            .unwrap()
            .into_iter()
            .find(|edge_type| edge_type.name == "custom")
            .expect("edge type stored");
        assert_eq!(cleared.source_kinds, None);
        assert_eq!(cleared.destination_kinds.as_deref(), Some("goal"));
    }

    #[tokio::test]
    async fn rejects_contract_tightening_that_orphans_existing_edges() {
        let service = service().await;
        let alpha = service
            .create_node_kind(&node_kind("alpha"))
            .await
            .unwrap()
            .id;
        let beta = service
            .create_node_kind(&node_kind("beta"))
            .await
            .unwrap()
            .id;
        let links = service
            .create_edge_type(&edge_type("links", "alpha -> beta"))
            .await
            .unwrap()
            .id;
        let source = service
            .create_node(&node(alpha, "na", ""), &[])
            .await
            .unwrap()
            .0;
        let destination = service
            .create_node(&node(beta, "nb", ""), &[])
            .await
            .unwrap()
            .0;
        service
            .add_edge(&Edge {
                source: source.id,
                destination: destination.id,
                edge_type_id: links,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let error = service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![PackEdgeType {
                    name: "links".to_string(),
                    description: None,
                    resolves: None,
                    single_outgoing: None,
                    acyclic: None,
                    source_kinds: Some(vec!["beta".to_string()]),
                    destination_kinds: Some(vec!["beta".to_string()]),
                }],
            })
            .await
            .unwrap_err();
        assert!(rejection(error).contains("existing edges in violation"));

        let stored = service
            .list_edge_types()
            .await
            .unwrap()
            .into_iter()
            .find(|edge_type| edge_type.name == "links")
            .expect("edge type stored");
        assert_eq!(stored.source_kinds, None);
    }

    #[tokio::test]
    async fn rejects_single_outgoing_tightening_with_existing_fan_out() {
        let service = service().await;
        let alpha = service
            .create_node_kind(&node_kind("alpha"))
            .await
            .unwrap()
            .id;
        let beta = service
            .create_node_kind(&node_kind("beta"))
            .await
            .unwrap()
            .id;
        let fan = service
            .create_edge_type(&edge_type("fan", "alpha -> beta"))
            .await
            .unwrap()
            .id;
        let source = service
            .create_node(&node(alpha, "na", ""), &[])
            .await
            .unwrap()
            .0;
        for name in ["first", "second"] {
            let destination = service
                .create_node(&node(beta, name, ""), &[])
                .await
                .unwrap()
                .0;
            service
                .add_edge(&Edge {
                    source: source.id,
                    destination: destination.id,
                    edge_type_id: fan,
                    created_at: String::new(),
                })
                .await
                .unwrap();
        }

        let error = service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![PackEdgeType {
                    name: "fan".to_string(),
                    description: None,
                    resolves: None,
                    single_outgoing: Some(true),
                    acyclic: None,
                    source_kinds: None,
                    destination_kinds: None,
                }],
            })
            .await
            .unwrap_err();
        assert!(rejection(error).contains("outgoing edges"));
    }

    #[tokio::test]
    async fn rejects_duplicate_and_undefined_pack_entries() {
        let service = service().await;
        assert!(matches!(
            service
                .apply_pack(&PackDraft {
                    node_kinds: vec![pack_kind("widget", None), pack_kind("widget", None)],
                    edge_types: vec![],
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let ghost = pack_edge_type("widget_part", "widget", "ghost");
        let error = service
            .apply_pack(&PackDraft {
                node_kinds: vec![pack_kind("widget", None)],
                edge_types: vec![ghost],
            })
            .await
            .unwrap_err();
        assert!(rejection(error).contains("ghost"));
    }

    #[tokio::test]
    async fn rejects_invalid_vocabulary_names() {
        let service = service().await;
        for name in ["", "   ", "a,b", "a\nb"] {
            assert!(
                matches!(
                    service.create_node_kind(&node_kind(name)).await,
                    Err(ServiceError::UnprocessableEntity(_))
                ),
                "{name:?} must be rejected"
            );
        }
        let long = "n".repeat(MAX_VOCABULARY_NAME_LENGTH + 1);
        assert!(matches!(
            service.create_node_kind(&node_kind(&long)).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service.create_edge_type(&edge_type("a,b", "")).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .apply_pack(&PackDraft {
                    node_kinds: vec![pack_kind("a,b", None)],
                    edge_types: vec![],
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn pack_never_deletes_omitted_vocabulary() {
        let service = service().await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![pack_kind("widget", Some("a widget"))],
                edge_types: vec![pack_edge_type("widget_part", "widget", "widget")],
            })
            .await
            .unwrap();

        let report = service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![],
            })
            .await
            .unwrap();
        assert_eq!(report.created(), 0);
        assert!(
            service
                .list_node_kinds()
                .await
                .unwrap()
                .iter()
                .any(|existing| existing.name == "widget")
        );
        assert!(
            service
                .list_edge_types()
                .await
                .unwrap()
                .iter()
                .any(|existing| existing.name == "widget_part")
        );
    }

    fn temp_sqlite(prefix: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}.sqlite", std::process::id()))
    }

    fn remove_sqlite(path: &std::path::Path) {
        std::fs::remove_file(path).ok();
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::fs::remove_file(path.with_file_name(format!("{file}-wal"))).ok();
        std::fs::remove_file(path.with_file_name(format!("{file}-shm"))).ok();
    }

    async fn file_service(path: &std::path::Path) -> GraphService {
        let config = crate::config::DatabaseConfig {
            path: path.to_string_lossy().into_owned(),
            max_connections: 5,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");
        let repository: Arc<dyn GraphRepository> = Arc::new(SqliteGraphRepository::new(pool));
        crate::test_support::seed_vocabulary(&repository).await;
        GraphService::new(repository, crate::config::PagingConfig::default())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_applies_serialize_as_idempotent_writes() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-pack-{}-{unique}.sqlite",
            std::process::id()
        ));
        let service = file_service(&path).await;
        let pack = PackDraft {
            node_kinds: vec![pack_kind("race", Some("race kind"))],
            edge_types: vec![pack_edge_type("race_edge", "race", "race")],
        };

        let mut handles = Vec::new();
        for _ in 0..8 {
            let service = service.clone();
            let pack = pack.clone();
            handles.push(tokio::spawn(async move { service.apply_pack(&pack).await }));
        }
        let mut created = 0;
        let mut unchanged = 0;
        for handle in handles {
            let report = handle.await.unwrap().expect("apply succeeds");
            created += report.created();
            unchanged += report.unchanged();
        }
        assert_eq!(created, 2);
        assert_eq!(unchanged, 14);

        drop(service);
        remove_sqlite(&path);
    }

    fn import_node(reference: Option<&str>, kind: &str, name: &str) -> ImportNode {
        ImportNode {
            reference: reference.map(str::to_string),
            id: None,
            kind: kind.to_string(),
            name: name.to_string(),
            description: None,
            content: None,
            tags: None,
            properties: None,
        }
    }

    fn import_draft(nodes: Vec<ImportNode>, edges: Vec<ImportEdge>) -> ImportDraft {
        ImportDraft {
            on_conflict: ImportConflict::Update,
            nodes,
            edges,
        }
    }

    #[tokio::test]
    async fn imports_nodes_and_edges_with_refs() {
        let service = service().await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let goal = service
            .create_node(&node(goal_kind, "ship import", ""), &[])
            .await
            .unwrap()
            .0;

        let mut task = import_node(Some("t1"), "task", "write import");
        task.description = Some("desc".to_string());
        task.content = Some("content".to_string());
        task.tags = Some(vec!["Repo:X".to_string()]);
        let draft = import_draft(
            vec![task],
            vec![ImportEdge {
                source: ImportRef::Ref("t1".to_string()),
                destination: ImportRef::Id(goal.id),
                edge_type: "part_of".to_string(),
                properties: None,
            }],
        );

        let report = service.import_graph(&draft).await.unwrap();
        assert_eq!(report.nodes_created(), 1);
        assert_eq!(report.edges_created(), 1);
        assert_eq!(report.nodes[0].reference.as_deref(), Some("t1"));

        let stored = service.get_node(report.nodes[0].id).await.unwrap();
        assert_eq!(stored.name, "write import");
        assert_eq!(stored.content, "content");
        assert_eq!(
            service.node_tags(report.nodes[0].id).await.unwrap(),
            vec!["repo:x"]
        );
    }

    #[tokio::test]
    async fn reimporting_converges() {
        let service = service().await;
        let mut decision = import_node(None, "decision", "use sqlite");
        decision.tags = Some(vec!["topic:storage".to_string()]);
        let draft = import_draft(vec![decision], vec![]);

        let first = service.import_graph(&draft).await.unwrap();
        assert_eq!(first.nodes_created(), 1);

        let second = service.import_graph(&draft).await.unwrap();
        assert_eq!(second.nodes_created(), 0);
        assert_eq!(second.nodes_unchanged(), 1);
        assert_eq!(second.nodes[0].id, first.nodes[0].id);
    }

    #[tokio::test]
    async fn import_updates_omitted_fields_and_skip_mode() {
        let service = service().await;
        let mut decision = import_node(None, "decision", "use sqlite");
        decision.description = Some("first".to_string());
        decision.content = Some("keep me".to_string());
        decision.tags = Some(vec!["one".to_string()]);
        let created = service
            .import_graph(&import_draft(vec![decision], vec![]))
            .await
            .unwrap();

        let mut update = import_node(None, "decision", "use sqlite");
        update.description = Some("second".to_string());
        update.tags = Some(vec!["two".to_string()]);
        let updated = service
            .import_graph(&import_draft(vec![update], vec![]))
            .await
            .unwrap();
        assert_eq!(updated.nodes_updated(), 1);
        let stored = service.get_node(created.nodes[0].id).await.unwrap();
        assert_eq!(stored.description, "second");
        assert_eq!(stored.content, "keep me");
        assert_eq!(
            service.node_tags(created.nodes[0].id).await.unwrap(),
            vec!["two"]
        );

        let mut skipped = import_node(None, "decision", "use sqlite");
        skipped.description = Some("third".to_string());
        let report = service
            .import_graph(&ImportDraft {
                on_conflict: ImportConflict::Skip,
                nodes: vec![skipped],
                edges: vec![],
            })
            .await
            .unwrap();
        assert_eq!(report.nodes_skipped(), 1);
        assert_eq!(
            service
                .get_node(created.nodes[0].id)
                .await
                .unwrap()
                .description,
            "second"
        );
    }

    #[tokio::test]
    async fn import_rejects_invalid_drafts() {
        let service = service().await;
        let cases = vec![
            import_draft(
                vec![
                    import_node(Some("dup"), "task", "a"),
                    import_node(Some("dup"), "task", "b"),
                ],
                vec![],
            ),
            import_draft(
                vec![
                    import_node(None, "task", "same"),
                    import_node(None, "task", "same"),
                ],
                vec![],
            ),
            import_draft(vec![import_node(None, "task", "   ")], vec![]),
            import_draft(vec![import_node(None, "ghost", "x")], vec![]),
            import_draft(
                vec![import_node(Some("a"), "task", "x")],
                vec![ImportEdge {
                    source: ImportRef::Ref("a".to_string()),
                    destination: ImportRef::Ref("a".to_string()),
                    edge_type: "ghost".to_string(),
                    properties: None,
                }],
            ),
            import_draft(
                vec![import_node(Some("a"), "task", "x")],
                vec![ImportEdge {
                    source: ImportRef::Ref("missing".to_string()),
                    destination: ImportRef::Ref("a".to_string()),
                    edge_type: "part_of".to_string(),
                    properties: None,
                }],
            ),
            import_draft(
                vec![import_node(Some("a"), "task", "x")],
                vec![ImportEdge {
                    source: ImportRef::Ref("a".to_string()),
                    destination: ImportRef::Id(999),
                    edge_type: "part_of".to_string(),
                    properties: None,
                }],
            ),
        ];
        for draft in cases {
            assert!(
                matches!(
                    service.import_graph(&draft).await,
                    Err(ServiceError::UnprocessableEntity(_))
                ),
                "draft was accepted: {draft:?}"
            );
        }
    }

    #[tokio::test]
    async fn import_contract_violation_rolls_back_the_batch() {
        let service = service().await;
        let before = service
            .list_nodes(&NodeFilter::default())
            .await
            .unwrap()
            .len();
        let draft = import_draft(
            vec![import_node(Some("d"), "decision", "should not persist")],
            vec![ImportEdge {
                source: ImportRef::Ref("d".to_string()),
                destination: ImportRef::Id(1),
                edge_type: "part_of".to_string(),
                properties: None,
            }],
        );
        let error = service.import_graph(&draft).await.unwrap_err();
        assert!(rejection(error).contains("does not accept"));

        let after = service
            .list_nodes(&NodeFilter::default())
            .await
            .unwrap()
            .len();
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn import_rejects_a_second_single_outgoing_edge() {
        let service = service().await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let goal = service
            .create_node(&node(goal_kind, "anchored", ""), &[])
            .await
            .unwrap()
            .0;
        let first_repo = service
            .create_node(&node(repo_kind, "acme/one", ""), &[])
            .await
            .unwrap()
            .0;
        let second_repo = service
            .create_node(&node(repo_kind, "acme/two", ""), &[])
            .await
            .unwrap()
            .0;
        service
            .add_edge(&Edge {
                source: goal.id,
                destination: first_repo.id,
                edge_type_id: seeded_edge_type(&service, "belongs_to").await,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let error = service
            .import_graph(&import_draft(
                vec![],
                vec![ImportEdge {
                    source: ImportRef::Id(goal.id),
                    destination: ImportRef::Id(second_repo.id),
                    edge_type: "belongs_to".to_string(),
                    properties: None,
                }],
            ))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("one outgoing edge"));
    }

    #[tokio::test]
    async fn duplicate_node_creation_is_a_conflict() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        service
            .create_node(&node(kind_id, "Ada", ""), &[])
            .await
            .unwrap();
        assert!(matches!(
            service.create_node(&node(kind_id, "Ada", ""), &[]).await,
            Err(ServiceError::Conflict(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_imports_converge() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-import-{}-{unique}.sqlite",
            std::process::id()
        ));
        let service = file_service(&path).await;
        let mut decision = import_node(Some("d"), "decision", "shared");
        decision.description = Some("shared context".to_string());
        let draft = import_draft(vec![decision], vec![]);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let service = service.clone();
            let draft = draft.clone();
            handles.push(tokio::spawn(
                async move { service.import_graph(&draft).await },
            ));
        }
        let mut created = 0;
        let mut unchanged = 0;
        for handle in handles {
            let report = handle.await.unwrap().expect("import succeeds");
            created += report.nodes_created();
            unchanged += report.nodes_unchanged();
        }
        assert_eq!(created, 1);
        assert_eq!(unchanged, 7);

        drop(service);
        remove_sqlite(&path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_task_and_plan_writes_succeed() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-task-writes-{}-{unique}.sqlite",
            std::process::id()
        ));
        let service = file_service(&path).await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let mut handles = Vec::new();
        for index in 0..8 {
            let service = service.clone();
            handles.push(tokio::spawn(async move {
                let mut task = new_task(&format!("concurrent task {index}"));
                task.parent = Some(goal);
                task.tags = Some(vec![format!("tag{index}")]);
                service.create_task(&task).await
            }));
        }
        for handle in handles {
            handle
                .await
                .unwrap()
                .expect("concurrent task write succeeds");
        }

        let mut handles = Vec::new();
        for index in 0..8 {
            let service = service.clone();
            handles.push(tokio::spawn(async move {
                let mut task = plan_task(&format!("plan task {index}"), None, &[]);
                task.tags = Some(vec![format!("plan{index}")]);
                let mut draft = plan_draft(repo, vec![task]);
                let plan_goal = draft.goal.as_mut().expect("goal");
                plan_goal.name = format!("concurrent goal {index}");
                plan_goal.tags = Some(vec![format!("goal{index}")]);
                service.create_plan(&draft).await
            }));
        }
        for handle in handles {
            handle
                .await
                .unwrap()
                .expect("concurrent plan write succeeds");
        }

        let mut existing = new_task("upsert target");
        existing.parent = Some(goal);
        let target = service.create_task(&existing).await.unwrap().id;
        let mut handles = Vec::new();
        for index in 0..8 {
            let service = service.clone();
            handles.push(tokio::spawn(async move {
                let mut task = new_task("upsert target");
                task.id = target;
                task.parent = Some(goal);
                task.description = format!("update {index}");
                task.tags = Some(vec![format!("upsert{index}")]);
                service.create_task(&task).await
            }));
        }
        for handle in handles {
            handle
                .await
                .unwrap()
                .expect("concurrent task upsert succeeds");
        }

        drop(service);
        remove_sqlite(&path);
    }

    #[tokio::test]
    async fn rejects_duplicate_task_names_in_a_plan() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let mut first = plan_task("a", None, &[]);
        let mut second = plan_task("b", None, &[]);
        first.name = "write tests".to_string();
        second.name = "write tests".to_string();

        let error = service
            .create_plan(&PlanDraft {
                goal: Some(PlanGoalDraft {
                    id: 0,
                    name: "ship widgets".to_string(),
                    description: String::new(),
                    content: String::new(),
                    properties: None,
                    tags: None,
                }),
                goal_id: None,
                anchor: Some(PlanAnchorDraft {
                    node_id: repo,
                    edge_type: "belongs_to".to_string(),
                }),
                tasks: vec![first, second],
            })
            .await
            .unwrap_err();
        assert!(rejection(error).contains("share the name"));
    }

    #[tokio::test]
    async fn rejects_a_task_name_that_already_exists() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let mut first = plan_task("a", None, &[]);
        first.name = "shared task".to_string();
        service
            .create_plan(&PlanDraft {
                goal: Some(PlanGoalDraft {
                    id: 0,
                    name: "first goal".to_string(),
                    description: String::new(),
                    content: String::new(),
                    properties: None,
                    tags: None,
                }),
                goal_id: None,
                anchor: Some(PlanAnchorDraft {
                    node_id: repo,
                    edge_type: "belongs_to".to_string(),
                }),
                tasks: vec![first],
            })
            .await
            .unwrap();

        let mut second = plan_task("b", None, &[]);
        second.name = "shared task".to_string();
        let error = service
            .create_plan(&PlanDraft {
                goal: Some(PlanGoalDraft {
                    id: 0,
                    name: "second goal".to_string(),
                    description: String::new(),
                    content: String::new(),
                    properties: None,
                    tags: None,
                }),
                goal_id: None,
                anchor: Some(PlanAnchorDraft {
                    node_id: repo,
                    edge_type: "belongs_to".to_string(),
                }),
                tasks: vec![second],
            })
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(error.to_string().contains("unique per kind"));
    }

    #[tokio::test]
    async fn import_accepts_natural_key_edge_endpoints() {
        let service = service().await;
        service
            .import_graph(&import_draft(
                vec![import_node(None, "goal", "anchor goal")],
                vec![],
            ))
            .await
            .unwrap();

        let task = import_node(Some("t"), "task", "child");
        let report = service
            .import_graph(&import_draft(
                vec![task],
                vec![ImportEdge {
                    source: ImportRef::Ref("t".to_string()),
                    destination: ImportRef::NaturalKey {
                        kind: "goal".to_string(),
                        name: "anchor goal".to_string(),
                    },
                    edge_type: "part_of".to_string(),
                    properties: None,
                }],
            ))
            .await
            .unwrap();
        assert_eq!(report.nodes_created(), 1);
        assert_eq!(report.edges_created(), 1);
    }

    #[tokio::test]
    async fn import_rejects_an_unknown_natural_key() {
        let service = service().await;
        let error = service
            .import_graph(&import_draft(
                vec![import_node(Some("t"), "task", "child")],
                vec![ImportEdge {
                    source: ImportRef::Ref("t".to_string()),
                    destination: ImportRef::NaturalKey {
                        kind: "goal".to_string(),
                        name: "missing".to_string(),
                    },
                    edge_type: "part_of".to_string(),
                    properties: None,
                }],
            ))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("no node with kind"));
    }

    #[tokio::test]
    async fn import_trims_references() {
        let service = service().await;
        let report = service
            .import_graph(&import_draft(
                vec![
                    import_node(Some("g"), "goal", "trim goal"),
                    import_node(Some("t"), "task", "trim task"),
                ],
                vec![ImportEdge {
                    source: ImportRef::Ref("  t  ".to_string()),
                    destination: ImportRef::Ref(" g ".to_string()),
                    edge_type: "part_of".to_string(),
                    properties: None,
                }],
            ))
            .await
            .unwrap();
        assert_eq!(report.edges_created(), 1);
    }

    #[tokio::test]
    async fn rejects_invalid_node_names() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        for name in ["", "   ", "a\nb"] {
            assert!(
                matches!(
                    service.create_node(&node(kind_id, name, ""), &[]).await,
                    Err(ServiceError::UnprocessableEntity(_))
                ),
                "{name:?} must be rejected"
            );
        }
        let long = "n".repeat(MAX_NODE_NAME_LENGTH + 1);
        assert!(matches!(
            service.create_node(&node(kind_id, &long, ""), &[]).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .import_graph(&import_draft(
                    vec![import_node(None, "person", &long)],
                    vec![]
                ))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn updates_a_non_task_node_and_keeps_tags() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let (node, _) = service
            .create_node(
                &described_node(kind_id, "Ada", "first"),
                &["Math".to_string()],
            )
            .await
            .unwrap();

        let (updated, tags) = service
            .update_node(
                node.id,
                &NodePatch {
                    description: Some("second".to_string()),
                    content: Some("body".to_string()),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.id, node.id);
        assert_eq!(updated.kind_id, kind_id);
        assert_eq!(updated.name, "Ada");
        assert_eq!(updated.description, "second");
        assert_eq!(updated.content, "body");
        assert_eq!(tags, vec!["math"]);
        assert_ne!(updated.updated_at, node.updated_at);
    }

    #[tokio::test]
    async fn renames_a_node_and_keeps_its_edges() {
        let service = service().await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let task_kind = seeded_kind(&service, "task").await;
        let goal = service
            .create_node(&node(goal_kind, "goal", ""), &[])
            .await
            .unwrap()
            .0;
        let task = service
            .create_node(&node(task_kind, "task", ""), &[])
            .await
            .unwrap()
            .0;
        let part_of = seeded_edge_type(&service, "part_of").await;
        service
            .add_edge(&Edge {
                source: task.id,
                destination: goal.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let (renamed, _) = service
            .update_node(
                task.id,
                &NodePatch {
                    name: Some("renamed task".to_string()),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(renamed.name, "renamed task");

        let neighbors = service
            .neighbors(task.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node.id, goal.id);
    }

    #[tokio::test]
    async fn rejects_a_rename_to_an_existing_name() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let first = service
            .create_node(&node(kind_id, "Ada", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "Bob", ""), &[])
            .await
            .unwrap()
            .0;

        let error = service
            .update_node(
                second.id,
                &NodePatch {
                    name: Some("Ada".to_string()),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::Conflict(_)));
        assert_eq!(service.get_node(second.id).await.unwrap().name, "Bob");
        assert_eq!(service.get_node(first.id).await.unwrap().name, "Ada");
    }

    #[tokio::test]
    async fn replaces_and_clears_tags_through_update() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let (node, _) = service
            .create_node(&node(kind_id, "Ada", ""), &["one".to_string()])
            .await
            .unwrap();

        let (_, tags) = service
            .update_node(
                node.id,
                &NodePatch {
                    tags: Some(vec!["Two".to_string()]),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(tags, vec!["two"]);

        let (_, tags) = service
            .update_node(
                node.id,
                &NodePatch {
                    tags: Some(vec![]),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert!(tags.is_empty());
    }

    #[tokio::test]
    async fn rejects_empty_or_missing_node_updates() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let existing = service
            .create_node(&node(kind_id, "Ada", ""), &[])
            .await
            .unwrap()
            .0;
        assert!(matches!(
            service
                .update_node(existing.id, &NodePatch::default())
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .update_node(
                    9999,
                    &NodePatch {
                        description: Some("x".to_string()),
                        ..NodePatch::default()
                    }
                )
                .await,
            Err(ServiceError::EntityNotFound)
        ));
        assert!(matches!(
            service.update_node(9999, &NodePatch::default()).await,
            Err(ServiceError::EntityNotFound)
        ));
    }

    #[tokio::test]
    async fn tag_only_update_bumps_updated_at() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let (node, _) = service
            .create_node(&node(kind_id, "Ada", ""), &["one".to_string()])
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let (updated, tags) = service
            .update_node(
                node.id,
                &NodePatch {
                    tags: Some(vec!["two".to_string()]),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(tags, vec!["two"]);
        assert_ne!(updated.updated_at, node.updated_at);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_task_rename_and_patch_preserve_both_fields() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-task-update-{}-{unique}.sqlite",
            std::process::id()
        ));
        let service = file_service(&path).await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let task_kind = seeded_kind(&service, "task").await;
        let part_of = seeded_edge_type(&service, "part_of").await;
        let repo = service
            .create_node(&node(repo_kind, "acme/widget", ""), &[])
            .await
            .unwrap()
            .0;
        let goal = service
            .create_node(&node(goal_kind, "race goal", ""), &[])
            .await
            .unwrap()
            .0;
        let task = service
            .create_node(&node(task_kind, "race task 0", ""), &[])
            .await
            .unwrap()
            .0;
        service
            .add_edge(&Edge {
                source: task.id,
                destination: goal.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap();
        let _ = repo;

        for index in 0..20 {
            let name = format!("race task {index}");
            let description = format!("version-{index}");
            let left = {
                let service = service.clone();
                let name = name.clone();
                tokio::spawn(async move {
                    service
                        .update_task(
                            task.id,
                            &TaskPatch {
                                name: Some(name),
                                description: None,
                                content: None,
                                properties: None,
                                tags: None,
                            },
                        )
                        .await
                })
            };
            let right = {
                let service = service.clone();
                let description = description.clone();
                tokio::spawn(async move {
                    service
                        .update_node(
                            task.id,
                            &NodePatch {
                                description: Some(description),
                                ..NodePatch::default()
                            },
                        )
                        .await
                })
            };
            left.await.unwrap().expect("task rename");
            right.await.unwrap().expect("node patch");

            let current = service.get_node(task.id).await.unwrap();
            assert_eq!(current.name, name);
            assert_eq!(current.description, description);
        }

        drop(service);
        remove_sqlite(&path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_renames_to_the_same_name_conflict() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-update-{}-{unique}.sqlite",
            std::process::id()
        ));
        let service = file_service(&path).await;
        service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap();
        let kind_id = seeded_kind(&service, "person").await;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;
        let patch = NodePatch {
            name: Some("winner".to_string()),
            ..NodePatch::default()
        };

        let left = {
            let service = service.clone();
            let patch = patch.clone();
            tokio::spawn(async move { service.update_node(first.id, &patch).await })
        };
        let right = {
            let service = service.clone();
            let patch = patch.clone();
            tokio::spawn(async move { service.update_node(second.id, &patch).await })
        };
        let left = left.await.unwrap();
        let right = right.await.unwrap();
        let wins = [left.is_ok(), right.is_ok()]
            .iter()
            .filter(|ok| **ok)
            .count();
        assert_eq!(wins, 1);
        let conflicts = [&left, &right]
            .iter()
            .filter(|result| matches!(result, Err(ServiceError::Conflict(_))))
            .count();
        assert_eq!(conflicts, 1);

        let named: Vec<String> = service
            .list_nodes(&NodeFilter {
                properties: Default::default(),
                kind: Some("person".to_string()),
                ..NodeFilter::default()
            })
            .await
            .unwrap()
            .into_iter()
            .map(|node| node.name)
            .collect();
        assert!(named.contains(&"winner".to_string()));
        assert_eq!(named.len(), 2);

        drop(service);
        remove_sqlite(&path);
    }

    #[tokio::test]
    async fn node_properties_are_set_replaced_and_cleared() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let properties = serde_json::json!({"confidence": 0.9, "source": "docs"});
        let (node, _) = service
            .create_node_with_properties(&node(kind_id, "Ada", ""), &[], Some(&properties))
            .await
            .unwrap();
        assert_eq!(service.node_properties(node.id).await.unwrap(), properties);

        service
            .update_node(
                node.id,
                &NodePatch {
                    description: Some("kept".to_string()),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(service.node_properties(node.id).await.unwrap(), properties);

        let replaced = serde_json::json!({"status": "verified"});
        let (updated, _) = service
            .update_node(
                node.id,
                &NodePatch {
                    properties: Some(replaced.clone()),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(service.node_properties(node.id).await.unwrap(), replaced);
        assert_ne!(updated.updated_at, node.updated_at);

        service
            .update_node(
                node.id,
                &NodePatch {
                    properties: Some(serde_json::json!({})),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            service.node_properties(node.id).await.unwrap(),
            serde_json::json!({})
        );
    }

    #[tokio::test]
    async fn import_sets_preserves_and_clears_properties() {
        let service = service().await;
        let mut decision = import_node(Some("d"), "decision", "use sqlite");
        decision.properties = Some(serde_json::json!({"confidence": 0.8}));
        let report = service
            .import_graph(&import_draft(vec![decision], vec![]))
            .await
            .unwrap();
        let node_id = report.nodes[0].id;
        assert_eq!(
            service.node_properties(node_id).await.unwrap(),
            serde_json::json!({"confidence": 0.8})
        );

        let report = service
            .import_graph(&import_draft(
                vec![import_node(Some("d"), "decision", "use sqlite")],
                vec![],
            ))
            .await
            .unwrap();
        assert_eq!(report.nodes_unchanged(), 1);
        assert_eq!(
            service.node_properties(node_id).await.unwrap(),
            serde_json::json!({"confidence": 0.8})
        );

        let mut cleared = import_node(Some("d"), "decision", "use sqlite");
        cleared.properties = Some(serde_json::json!({}));
        service
            .import_graph(&import_draft(vec![cleared], vec![]))
            .await
            .unwrap();
        assert_eq!(
            service.node_properties(node_id).await.unwrap(),
            serde_json::json!({})
        );
    }

    #[tokio::test]
    async fn rejects_non_object_properties() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        assert!(matches!(
            service
                .create_node_with_properties(
                    &node(kind_id, "Ada", ""),
                    &[],
                    Some(&serde_json::json!([1])),
                )
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let (node, _) = service
            .create_node(&node(kind_id, "Bob", ""), &[])
            .await
            .unwrap();
        assert!(matches!(
            service
                .update_node(
                    node.id,
                    &NodePatch {
                        properties: Some(serde_json::json!("nope")),
                        ..NodePatch::default()
                    }
                )
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let mut invalid = import_node(None, "person", "Cara");
        invalid.properties = Some(serde_json::json!(3));
        assert!(matches!(
            service
                .import_graph(&import_draft(vec![invalid], vec![]))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    fn property_filter(value: serde_json::Value) -> NodeFilter {
        let properties = match value {
            serde_json::Value::Object(map) => map,
            _ => panic!("property filter must be an object"),
        };
        NodeFilter {
            properties,
            ..NodeFilter::default()
        }
    }

    #[tokio::test]
    async fn filters_nodes_by_property_values() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let verified = serde_json::json!({
            "status": "verified",
            "confidence": 0.9,
            "retried": true,
            "note": null,
            "scopes": ["a", "b"]
        });
        let first = service
            .create_node_with_properties(
                &described_node(kind_id, "verified", "portable database"),
                &[],
                Some(&verified),
            )
            .await
            .unwrap()
            .0;
        let second = service
            .create_node_with_properties(
                &described_node(kind_id, "draft", "portable notes"),
                &[],
                Some(&serde_json::json!({"status": "draft", "confidence": 0.4})),
            )
            .await
            .unwrap()
            .0;
        service
            .create_node(&described_node(kind_id, "plain", "portable"), &[])
            .await
            .unwrap();

        let ids = |filter: NodeFilter| {
            let service = service.clone();
            async move {
                service
                    .list_nodes(&filter)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|node| node.id)
                    .collect::<Vec<_>>()
            }
        };

        assert_eq!(
            ids(property_filter(serde_json::json!({"status": "verified"}))).await,
            vec![first.id]
        );
        assert_eq!(
            ids(property_filter(serde_json::json!({"confidence": 0.9}))).await,
            vec![first.id]
        );
        assert_eq!(
            ids(property_filter(serde_json::json!({"retried": true}))).await,
            vec![first.id]
        );
        assert_eq!(
            ids(property_filter(serde_json::json!({"note": null}))).await,
            vec![first.id]
        );
        assert_eq!(
            ids(property_filter(serde_json::json!({"scopes": ["a", "b"]}))).await,
            vec![first.id]
        );
        assert!(
            ids(property_filter(serde_json::json!({"missing": "x"})))
                .await
                .is_empty()
        );
        assert!(
            ids(property_filter(
                serde_json::json!({"status": "draft", "confidence": 0.9})
            ))
            .await
            .is_empty()
        );
        assert_eq!(
            ids(property_filter(serde_json::json!({"status": "draft"}))).await,
            vec![second.id]
        );

        let mut with_kind = property_filter(serde_json::json!({"status": "verified"}));
        with_kind.kind = Some("person".to_string());
        assert_eq!(ids(with_kind).await, vec![first.id]);

        let found = service
            .search(
                Some("portable"),
                &[],
                &property_filter(serde_json::json!({"status": "draft"})).properties,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(found.total, 1);
        assert_eq!(found.hits[0].node.id, second.id);
    }

    async fn relates_edge_type(service: &GraphService, kind_id: i64) -> i64 {
        let kind_name = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.id == kind_id)
            .expect("kind")
            .name;
        service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "relates".to_string(),
                description: "person -> person".to_string(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: Some(kind_name.clone()),
                destination_kinds: Some(kind_name),
                updated_at: String::new(),
            })
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn edge_properties_are_stored_and_returned() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let relates = relates_edge_type(&service, kind_id).await;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;

        service
            .add_edge_with_properties(
                &Edge {
                    source: first.id,
                    destination: second.id,
                    edge_type_id: relates,
                    created_at: String::new(),
                },
                Some(&serde_json::json!({"weight": 2})),
            )
            .await
            .unwrap();
        let neighbors = service
            .neighbors(first.id, NeighborDirection::Outgoing, Some("relates"))
            .await
            .unwrap();
        assert_eq!(neighbors[0].edge_properties, r#"{"weight":2}"#);

        let third = service
            .create_node(&node(kind_id, "third", ""), &[])
            .await
            .unwrap()
            .0;
        service
            .add_edge_with_properties(
                &Edge {
                    source: first.id,
                    destination: third.id,
                    edge_type_id: relates,
                    created_at: String::new(),
                },
                Some(&serde_json::json!({"weight": 9})),
            )
            .await
            .unwrap();
        let neighbors = service
            .neighbors(first.id, NeighborDirection::Outgoing, Some("relates"))
            .await
            .unwrap();
        let second_edge = neighbors
            .iter()
            .find(|neighbor| neighbor.node.id == second.id)
            .expect("edge kept");
        assert_eq!(second_edge.edge_properties, r#"{"weight":2}"#);
    }

    #[tokio::test]
    async fn edge_properties_default_to_an_empty_object() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let relates = relates_edge_type(&service, kind_id).await;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;
        service
            .add_edge(&Edge {
                source: first.id,
                destination: second.id,
                edge_type_id: relates,
                created_at: String::new(),
            })
            .await
            .unwrap();
        let neighbors = service
            .neighbors(first.id, NeighborDirection::Outgoing, Some("relates"))
            .await
            .unwrap();
        assert_eq!(neighbors[0].edge_properties, "{}");
    }

    #[tokio::test]
    async fn import_replaces_edge_properties_on_an_existing_edge() {
        let service = service().await;
        let mut edge = ImportEdge {
            source: ImportRef::Ref("t".to_string()),
            destination: ImportRef::Ref("g".to_string()),
            edge_type: "part_of".to_string(),
            properties: Some(serde_json::json!({"weight": 1})),
        };
        let draft = import_draft(
            vec![
                import_node(Some("t"), "task", "child"),
                import_node(Some("g"), "goal", "parent"),
            ],
            vec![edge.clone()],
        );
        let report = service.import_graph(&draft).await.unwrap();
        assert_eq!(report.edges_created(), 1);
        let task_id = report.nodes[0].id;
        let neighbors = service
            .neighbors(task_id, NeighborDirection::Outgoing, Some("part_of"))
            .await
            .unwrap();
        assert_eq!(neighbors[0].edge_properties, r#"{"weight":1}"#);

        edge.properties = Some(serde_json::json!({"weight": 2}));
        let report = service
            .import_graph(&import_draft(
                vec![
                    import_node(Some("t"), "task", "child"),
                    import_node(Some("g"), "goal", "parent"),
                ],
                vec![edge],
            ))
            .await
            .unwrap();
        assert_eq!(report.edges_existing(), 1);
        let neighbors = service
            .neighbors(task_id, NeighborDirection::Outgoing, Some("part_of"))
            .await
            .unwrap();
        assert_eq!(neighbors[0].edge_properties, r#"{"weight":2}"#);
    }

    #[tokio::test]
    async fn rejects_non_object_edge_properties() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let relates = relates_edge_type(&service, kind_id).await;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;
        assert!(matches!(
            service
                .add_edge_with_properties(
                    &Edge {
                        source: first.id,
                        destination: second.id,
                        edge_type_id: relates,
                        created_at: String::new(),
                    },
                    Some(&serde_json::json!([1])),
                )
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));

        let edge = ImportEdge {
            source: ImportRef::Ref("a".to_string()),
            destination: ImportRef::Ref("b".to_string()),
            edge_type: "relates".to_string(),
            properties: Some(serde_json::json!("nope")),
        };
        let report = service
            .import_graph(&import_draft(
                vec![
                    import_node(Some("a"), "person", "a"),
                    import_node(Some("b"), "person", "b"),
                ],
                vec![edge],
            ))
            .await;
        assert!(matches!(report, Err(ServiceError::UnprocessableEntity(_))));
    }

    #[tokio::test]
    async fn updates_and_clears_edge_properties() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let relates = relates_edge_type(&service, kind_id).await;
        let first = service
            .create_node(&node(kind_id, "first", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "second", ""), &[])
            .await
            .unwrap()
            .0;
        let edge = Edge {
            source: first.id,
            destination: second.id,
            edge_type_id: relates,
            created_at: String::new(),
        };
        service
            .add_edge_with_properties(&edge, Some(&serde_json::json!({"weight": 1})))
            .await
            .unwrap();

        service
            .set_edge_properties(&edge, &serde_json::json!({"weight": 2, "tag": "a"}))
            .await
            .unwrap();
        let neighbors = service
            .neighbors(first.id, NeighborDirection::Outgoing, Some("relates"))
            .await
            .unwrap();
        let stored: serde_json::Value =
            serde_json::from_str(&neighbors[0].edge_properties).unwrap();
        assert_eq!(stored, serde_json::json!({"weight": 2, "tag": "a"}));

        service
            .set_edge_properties(&edge, &serde_json::json!({}))
            .await
            .unwrap();
        let neighbors = service
            .neighbors(first.id, NeighborDirection::Outgoing, Some("relates"))
            .await
            .unwrap();
        assert_eq!(neighbors[0].edge_properties, "{}");

        assert!(matches!(
            service
                .set_edge_properties(
                    &Edge {
                        source: second.id,
                        destination: first.id,
                        edge_type_id: relates,
                        created_at: String::new(),
                    },
                    &serde_json::json!({"weight": 3}),
                )
                .await,
            Err(ServiceError::EntityNotFound)
        ));
        assert!(matches!(
            service
                .set_edge_properties(&edge, &serde_json::json!([1]))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    fn property(key: &str, value_type: &str, required: bool) -> PackProperty {
        PackProperty {
            key: key.to_string(),
            value_type: Some(value_type.to_string()),
            required,
            indexed: false,
        }
    }

    fn widget_pack(properties: Option<Vec<PackProperty>>) -> PackDraft {
        PackDraft {
            node_kinds: vec![PackNodeKind {
                name: "widget".to_string(),
                description: Some("a widget".to_string()),
                unique_name: None,
                properties,
                strict_properties: None,
            }],
            edge_types: vec![],
        }
    }

    async fn widget_kind_id(service: &GraphService) -> i64 {
        service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "widget")
            .expect("widget kind")
            .id
    }

    #[tokio::test]
    async fn pack_declares_and_replaces_kind_properties() {
        let service = service().await;
        let report = service
            .apply_pack(&widget_pack(Some(vec![
                property("status", "string", true),
                property("confidence", "number", false),
            ])))
            .await
            .unwrap();
        assert_eq!(report.created(), 1);
        let kind_id = widget_kind_id(&service).await;
        let declarations = service
            .node_kind_properties_for_ids(&[kind_id])
            .await
            .unwrap();
        let declared = declarations.get(&kind_id).cloned().unwrap_or_default();
        assert_eq!(declared.len(), 2);
        assert_eq!(declared[0].key, "confidence");
        assert!(declared.iter().any(|property| property.key == "status"
            && property.required
            && property.value_type == PropertyType::String));

        let report = service
            .apply_pack(&widget_pack(Some(vec![property(
                "status", "string", false,
            )])))
            .await
            .unwrap();
        assert_eq!(report.updated(), 1);
        let kind = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "widget")
            .unwrap();
        assert_eq!(kind.version, 2);
        let declarations = service
            .node_kind_properties_for_ids(&[kind_id])
            .await
            .unwrap();
        let declared = declarations.get(&kind_id).cloned().unwrap_or_default();
        assert_eq!(declared.len(), 1);
        assert!(!declared[0].required);

        let report = service.apply_pack(&widget_pack(None)).await.unwrap();
        assert_eq!(report.unchanged(), 1);
        let declarations = service
            .node_kind_properties_for_ids(&[kind_id])
            .await
            .unwrap();
        assert_eq!(declarations.get(&kind_id).map(Vec::len).unwrap_or(0), 1);

        service
            .apply_pack(&widget_pack(Some(vec![])))
            .await
            .unwrap();
        let declarations = service
            .node_kind_properties_for_ids(&[kind_id])
            .await
            .unwrap();
        assert!(
            declarations
                .get(&kind_id)
                .map(Vec::is_empty)
                .unwrap_or(true)
        );
    }

    #[tokio::test]
    async fn declared_properties_are_enforced_on_writes() {
        let service = service().await;
        service
            .apply_pack(&widget_pack(Some(vec![
                property("status", "string", true),
                property("confidence", "number", false),
            ])))
            .await
            .unwrap();
        let kind_id = widget_kind_id(&service).await;

        let missing = service
            .create_node(&node(kind_id, "missing", ""), &[])
            .await
            .unwrap_err();
        assert!(rejection(missing).contains("required"));

        let wrong = service
            .create_node_with_properties(
                &node(kind_id, "wrong", ""),
                &[],
                Some(&serde_json::json!({"status": 1})),
            )
            .await
            .unwrap_err();
        assert!(rejection(wrong).contains("must be a string"));

        let created = service
            .create_node_with_properties(
                &node(kind_id, "ok", ""),
                &[],
                Some(&serde_json::json!({
                    "status": "new",
                    "confidence": 0.5,
                    "undeclared": true
                })),
            )
            .await
            .unwrap()
            .0;

        let patched = service
            .update_node(
                created.id,
                &NodePatch {
                    properties: Some(serde_json::json!({"confidence": 0.9})),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap_err();
        assert!(rejection(patched).contains("required"));

        let patched = service
            .update_node(
                created.id,
                &NodePatch {
                    properties: Some(serde_json::json!({"status": 5})),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap_err();
        assert!(rejection(patched).contains("must be a string"));

        let imported = import_node(None, "widget", "imported");
        let error = service
            .import_graph(&import_draft(vec![imported], vec![]))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("required"));

        let mut imported = import_node(None, "widget", "imported");
        imported.properties = Some(serde_json::json!({"status": "new"}));
        service
            .import_graph(&import_draft(vec![imported], vec![]))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_property_declarations() {
        let service = service().await;
        for properties in [
            vec![property("status", "strange", false)],
            vec![property(" ", "string", false)],
            vec![
                property("a", "string", false),
                property("a", "number", false),
            ],
        ] {
            assert!(matches!(
                service.apply_pack(&widget_pack(Some(properties))).await,
                Err(ServiceError::UnprocessableEntity(_))
            ));
        }
    }

    #[tokio::test]
    async fn tasks_and_plans_enforce_and_store_declared_properties() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: None,
                    properties: Some(vec![property("owner", "string", true)]),
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        let goal = anchored_goal(&service, repo).await;

        let mut draft = new_task("planned child");
        draft.parent = Some(goal);
        let error = service.create_task(&draft).await.unwrap_err();
        assert!(rejection(error).contains("owner"));

        draft.properties = Some(serde_json::json!({"owner": "alice"}));
        let created = service.create_task(&draft).await.unwrap();
        assert_eq!(
            service.node_properties(created.id).await.unwrap(),
            serde_json::json!({"owner": "alice"})
        );

        let mut task = plan_task("planned", None, &[]);
        let error = service
            .create_plan(&plan_draft(repo, vec![task.clone()]))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("owner"));

        task.properties = Some(serde_json::json!({"owner": "bob"}));
        let plan = service
            .create_plan(&plan_draft(repo, vec![task]))
            .await
            .unwrap();
        assert_eq!(
            service
                .node_properties(plan.tasks[0].node.id)
                .await
                .unwrap(),
            serde_json::json!({"owner": "bob"})
        );
    }

    #[tokio::test]
    async fn skip_mode_does_not_validate_properties_it_will_not_write() {
        let service = service().await;
        service
            .apply_pack(&widget_pack(Some(vec![property("owner", "string", true)])))
            .await
            .unwrap();
        let mut created = import_node(Some("w"), "widget", "w");
        created.properties = Some(serde_json::json!({"owner": "x"}));
        service
            .import_graph(&import_draft(vec![created], vec![]))
            .await
            .unwrap();

        let mut skipped = import_node(Some("w"), "widget", "w");
        skipped.properties = Some(serde_json::json!({}));
        let report = service
            .import_graph(&ImportDraft {
                on_conflict: ImportConflict::Skip,
                nodes: vec![skipped],
                edges: vec![],
            })
            .await
            .unwrap();
        assert_eq!(report.nodes_skipped(), 1);
    }

    async fn set_task_unique(service: &GraphService, unique: bool) {
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: Some(unique),
                    properties: None,
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn allows_duplicate_task_names_when_the_kind_opts_out() {
        let service = service().await;
        set_task_unique(&service, false).await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let mut first = new_task("write tests");
        first.parent = Some(goal);
        let mut second = new_task("write tests");
        second.parent = Some(goal);
        let left = service.create_task(&first).await.unwrap();
        let right = service.create_task(&second).await.unwrap();
        assert_eq!(left.name, right.name);

        let mut first = plan_task("a", None, &[]);
        let mut second = plan_task("b", None, &[]);
        first.name = "write tests".to_string();
        second.name = "write tests".to_string();
        let plan = service
            .create_plan(&plan_draft(repo, vec![first, second]))
            .await
            .unwrap();
        assert_eq!(plan.tasks.len(), 2);
    }

    #[tokio::test]
    async fn pack_toggles_unique_name_and_bumps_version() {
        let service = service().await;
        let before = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .unwrap();
        assert!(before.unique_name);

        set_task_unique(&service, false).await;
        let after = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .unwrap();
        assert!(!after.unique_name);
        assert_eq!(after.version, before.version + 1);

        let report = service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: Some(false),
                    properties: None,
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        assert_eq!(report.unchanged(), 1);

        set_task_unique(&service, true).await;
        let restored = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .unwrap();
        assert!(restored.unique_name);
        assert_eq!(restored.version, before.version + 2);
    }

    #[tokio::test]
    async fn import_rejects_an_ambiguous_natural_key() {
        let service = service().await;
        set_task_unique(&service, false).await;
        let task_kind = seeded_kind(&service, "task").await;
        service
            .create_node(&node(task_kind, "dup", ""), &[])
            .await
            .unwrap();
        service
            .create_node(&node(task_kind, "dup", ""), &[])
            .await
            .unwrap();

        let error = service
            .import_graph(&import_draft(
                vec![import_node(None, "task", "dup")],
                vec![],
            ))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("not unique"));
    }

    #[tokio::test]
    async fn rename_skips_conflict_for_non_unique_kinds() {
        let service = service().await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "note".to_string(),
                    description: Some("a note".to_string()),
                    unique_name: Some(false),
                    properties: None,
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        let kind_id = seeded_kind(&service, "note").await;
        let first = service
            .create_node(&node(kind_id, "same", ""), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&node(kind_id, "other", ""), &[])
            .await
            .unwrap()
            .0;

        let (renamed, _) = service
            .update_node(
                second.id,
                &NodePatch {
                    name: Some("same".to_string()),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(renamed.name, first.name);
        assert_eq!(service.get_node(second.id).await.unwrap().name, "same");
    }

    #[tokio::test]
    async fn tightening_unique_name_rejects_existing_duplicates() {
        let service = service().await;
        set_task_unique(&service, false).await;
        let task_kind = seeded_kind(&service, "task").await;
        service
            .create_node(&node(task_kind, "dup", ""), &[])
            .await
            .unwrap();
        service
            .create_node(&node(task_kind, "dup", ""), &[])
            .await
            .unwrap();

        let error = service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: Some(true),
                    properties: None,
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap_err();
        assert!(rejection(error).contains("duplicates"));

        let kind = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .unwrap();
        assert!(!kind.unique_name);
    }

    #[tokio::test]
    async fn ambiguous_natural_key_endpoint_is_rejected() {
        let service = service().await;
        set_task_unique(&service, false).await;
        let task_kind = seeded_kind(&service, "task").await;
        service
            .create_node(&node(task_kind, "dup", ""), &[])
            .await
            .unwrap();
        service
            .create_node(&node(task_kind, "dup", ""), &[])
            .await
            .unwrap();

        let error = service
            .import_graph(&import_draft(
                vec![import_node(Some("s"), "task", "source")],
                vec![ImportEdge {
                    source: ImportRef::Ref("s".to_string()),
                    destination: ImportRef::NaturalKey {
                        kind: "task".to_string(),
                        name: "dup".to_string(),
                    },
                    edge_type: "depends_on".to_string(),
                    properties: None,
                }],
            ))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("not unique"));
    }

    #[tokio::test]
    async fn import_updates_a_duplicate_named_node_by_id() {
        let service = service().await;
        set_task_unique(&service, false).await;
        let task_kind = seeded_kind(&service, "task").await;
        let first = service
            .create_node(&described_node(task_kind, "dup", "first"), &[])
            .await
            .unwrap()
            .0;
        let second = service
            .create_node(&described_node(task_kind, "dup", "second"), &[])
            .await
            .unwrap()
            .0;

        let mut imported = import_node(None, "task", "dup");
        imported.id = Some(second.id);
        imported.description = Some("third".to_string());
        let report = service
            .import_graph(&import_draft(vec![imported], vec![]))
            .await
            .unwrap();
        assert_eq!(report.nodes_updated(), 1);
        assert_eq!(report.nodes[0].id, second.id);
        assert_eq!(
            service.get_node(second.id).await.unwrap().description,
            "third"
        );
        assert_eq!(
            service.get_node(first.id).await.unwrap().description,
            "first"
        );
    }

    #[tokio::test]
    async fn import_creates_with_an_explicit_id() {
        let service = service().await;
        let mut imported = import_node(None, "task", "explicit");
        imported.id = Some(1000);
        let report = service
            .import_graph(&import_draft(vec![imported], vec![]))
            .await
            .unwrap();
        assert_eq!(report.nodes_created(), 1);
        assert_eq!(report.nodes[0].id, 1000);
        assert_eq!(service.get_node(1000).await.unwrap().name, "explicit");
    }

    #[tokio::test]
    async fn import_rejects_a_kind_change_by_id() {
        let service = service().await;
        let person_kind = seed_person(&service).await;
        let node = service
            .create_node(&described_node(person_kind, "Ada", ""), &[])
            .await
            .unwrap()
            .0;

        let mut imported = import_node(None, "task", "moved");
        imported.id = Some(node.id);
        let error = service
            .import_graph(&import_draft(vec![imported], vec![]))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("cannot change kind"));
        let stored = service.get_node(node.id).await.unwrap();
        assert_eq!(stored.kind_id, person_kind);
        assert_eq!(stored.name, "Ada");
    }

    #[tokio::test]
    async fn plan_upsert_rejects_a_kind_change() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let person_kind = seed_person(&service).await;
        let person = service
            .create_node(&described_node(person_kind, "Ada", ""), &[])
            .await
            .unwrap()
            .0;

        let mut task = plan_task("a", None, &[]);
        task.id = person.id;
        let error = service
            .create_plan(&plan_draft(repo, vec![task]))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("cannot change kind"));
        assert_eq!(
            service.get_node(person.id).await.unwrap().kind_id,
            person_kind
        );
    }

    #[tokio::test]
    async fn import_rejects_bad_ids() {
        let service = service().await;
        for id in [0, -1] {
            let mut node = import_node(None, "task", "bad");
            node.id = Some(id);
            assert!(matches!(
                service
                    .import_graph(&import_draft(vec![node], vec![]))
                    .await,
                Err(ServiceError::UnprocessableEntity(_))
            ));
        }

        let mut first = import_node(None, "task", "one");
        first.id = Some(5);
        let mut second = import_node(None, "task", "two");
        second.id = Some(5);
        assert!(matches!(
            service
                .import_graph(&import_draft(vec![first, second], vec![]))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn pack_maintains_property_indexes() {
        let (service, pool) = service_with_pool().await;
        let mut indexed = property("status", "string", false);
        indexed.indexed = true;
        service
            .apply_pack(&widget_pack(Some(vec![indexed])))
            .await
            .unwrap();

        let index_names = |pool: SqlitePool| async move {
            sqlx::query_scalar::<_, String>(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'index' AND name LIKE 'idx_node_prop_%'",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        let names = index_names(pool.clone()).await;
        assert_eq!(names.len(), 1);

        let kind_id = widget_kind_id(&service).await;
        service
            .create_node_with_properties(
                &node(kind_id, "indexed", ""),
                &[],
                Some(&serde_json::json!({"status": "on"})),
            )
            .await
            .unwrap();
        let found = service
            .list_nodes(&property_filter(serde_json::json!({"status": "on"})))
            .await
            .unwrap();
        assert_eq!(found.len(), 1);

        let plan = sqlx::query_as::<_, (i64, i64, i64, String)>(
            "EXPLAIN QUERY PLAN SELECT id FROM node \
             WHERE json_extract(properties, '$.\"status\"') = 'on'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let detail = plan
            .iter()
            .map(|(_, _, _, detail)| detail.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(detail.contains("USING INDEX"), "plan: {detail}");

        let mut moved = property("owner", "string", false);
        moved.indexed = true;
        service
            .apply_pack(&widget_pack(Some(vec![moved])))
            .await
            .unwrap();
        let names = index_names(pool.clone()).await;
        assert_eq!(names.len(), 1);
        let definition: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_node_prop_%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(definition.contains("owner"), "definition: {definition}");

        service
            .apply_pack(&widget_pack(Some(vec![])))
            .await
            .unwrap();
        assert!(index_names(pool).await.is_empty());
    }

    #[tokio::test]
    async fn updating_one_kind_does_not_drop_another_kinds_index() {
        let (service, pool) = service_with_pool().await;
        let mut indexed = property("status", "string", false);
        indexed.indexed = true;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![
                    PackNodeKind {
                        name: "repo".to_string(),
                        description: None,
                        unique_name: None,
                        properties: Some(vec![indexed.clone()]),
                        strict_properties: None,
                    },
                    PackNodeKind {
                        name: "db_view".to_string(),
                        description: None,
                        unique_name: None,
                        properties: Some(vec![indexed.clone()]),
                        strict_properties: None,
                    },
                ],
                edge_types: vec![],
            })
            .await
            .unwrap();

        let names = || {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, String>(
                    "SELECT name FROM sqlite_master \
                     WHERE type = 'index' AND name GLOB 'idx_node_prop_*'",
                )
                .fetch_all(&pool)
                .await
                .unwrap()
            }
        };
        assert_eq!(names().await.len(), 2);

        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "repo".to_string(),
                    description: None,
                    unique_name: None,
                    properties: Some(vec![indexed, property("owner", "string", false)]),
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();

        let remaining = names().await;
        assert_eq!(remaining.len(), 2, "indexes: {remaining:?}");
        assert!(
            remaining
                .iter()
                .any(|name| name.starts_with("idx_node_prop_10_")),
            "kind 10 index dropped: {remaining:?}"
        );
        assert!(
            remaining
                .iter()
                .any(|name| name.starts_with("idx_node_prop_1_")),
            "kind 1 index missing: {remaining:?}"
        );
    }

    #[tokio::test]
    async fn rejects_double_quotes_in_property_keys() {
        let (service, _pool) = service_with_pool().await;
        let bad = property("a\"b", "string", false);
        let error = service
            .apply_pack(&widget_pack(Some(vec![bad])))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("double quote"));
    }

    #[tokio::test]
    async fn strict_kinds_reject_undeclared_properties() {
        let (service, _pool) = service_with_pool().await;
        let mut pack = widget_pack(Some(vec![property("status", "string", true)]));
        pack.node_kinds[0].strict_properties = Some(true);
        service.apply_pack(&pack).await.unwrap();
        let kind_id = widget_kind_id(&service).await;

        let undeclared = service
            .create_node_with_properties(
                &node(kind_id, "bad", ""),
                &[],
                Some(&serde_json::json!({"status": "x", "extra": 1})),
            )
            .await
            .unwrap_err();
        assert!(rejection(undeclared).contains("not declared"));

        let created = service
            .create_node_with_properties(
                &node(kind_id, "good", ""),
                &[],
                Some(&serde_json::json!({"status": "x"})),
            )
            .await
            .unwrap()
            .0;

        let patched = service
            .update_node(
                created.id,
                &NodePatch {
                    properties: Some(serde_json::json!({"status": "y", "extra": 1})),
                    ..NodePatch::default()
                },
            )
            .await
            .unwrap_err();
        assert!(rejection(patched).contains("not declared"));

        let mut imported = import_node(None, "widget", "imported");
        imported.properties = Some(serde_json::json!({"status": "x", "extra": 1}));
        let imported = service
            .import_graph(&import_draft(vec![imported], vec![]))
            .await
            .unwrap_err();
        assert!(rejection(imported).contains("not declared"));

        let mut accepted = import_node(None, "widget", "imported");
        accepted.properties = Some(serde_json::json!({"status": "x"}));
        service
            .import_graph(&import_draft(vec![accepted], vec![]))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn plan_tasks_respect_strict_property_kinds() {
        let service = service().await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: None,
                    strict_properties: Some(true),
                    properties: Some(vec![property("owner", "string", false)]),
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let mut task = new_task("strict task");
        task.parent = Some(goal);
        task.properties = Some(serde_json::json!({"extra": 1}));
        let error = service.create_task(&task).await.unwrap_err();
        assert!(rejection(error).contains("not declared"));

        task.properties = Some(serde_json::json!({"owner": "a"}));
        service.create_task(&task).await.unwrap();

        let mut plan_task = plan_task("a", None, &[]);
        plan_task.properties = Some(serde_json::json!({"extra": 1}));
        let error = service
            .create_plan(&plan_draft(repo, vec![plan_task]))
            .await
            .unwrap_err();
        assert!(rejection(error).contains("not declared"));
    }

    #[tokio::test]
    async fn pack_toggles_strict_properties_and_bumps_version() {
        let service = service().await;
        let before = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .unwrap();
        assert!(!before.strict_properties);

        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: None,
                    strict_properties: Some(true),
                    properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        let after = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .unwrap();
        assert!(after.strict_properties);
        assert_eq!(after.version, before.version + 1);

        let report = service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "task".to_string(),
                    description: None,
                    unique_name: None,
                    strict_properties: Some(true),
                    properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        assert_eq!(report.unchanged(), 1);
    }

    #[tokio::test]
    async fn tightening_declarations_rejects_existing_violations() {
        let service = service().await;
        service.apply_pack(&widget_pack(None)).await.unwrap();
        let kind_id = widget_kind_id(&service).await;
        service
            .create_node_with_properties(
                &node(kind_id, "wrong-type", ""),
                &[],
                Some(&serde_json::json!({"status": 1})),
            )
            .await
            .unwrap();
        service
            .create_node_with_properties(&node(kind_id, "missing", ""), &[], None)
            .await
            .unwrap();

        let error = service
            .apply_pack(&widget_pack(Some(vec![property("status", "string", true)])))
            .await
            .unwrap_err();
        let message = rejection(error);
        assert!(message.contains("cannot tighten"), "message: {message}");

        let declarations = service
            .node_kind_properties_for_ids(&[kind_id])
            .await
            .unwrap();
        assert!(
            declarations
                .get(&kind_id)
                .map(Vec::is_empty)
                .unwrap_or(true)
        );

        let nodes = service
            .list_nodes(&NodeFilter {
                kind: Some("widget".to_string()),
                ..NodeFilter::default()
            })
            .await
            .unwrap();
        for node in &nodes {
            service
                .update_node(
                    node.id,
                    &NodePatch {
                        properties: Some(serde_json::json!({"status": "x"})),
                        ..NodePatch::default()
                    },
                )
                .await
                .unwrap();
        }
        service
            .apply_pack(&widget_pack(Some(vec![property("status", "string", true)])))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn tightening_strict_rejects_existing_undeclared_keys() {
        let service = service().await;
        service.apply_pack(&widget_pack(None)).await.unwrap();
        let kind_id = widget_kind_id(&service).await;
        service
            .create_node_with_properties(
                &node(kind_id, "legacy", ""),
                &[],
                Some(&serde_json::json!({"extra": 1})),
            )
            .await
            .unwrap();

        let mut pack = widget_pack(None);
        pack.node_kinds[0].strict_properties = Some(true);
        let error = service.apply_pack(&pack).await.unwrap_err();
        let message = rejection(error);
        assert!(message.contains("cannot tighten"), "message: {message}");

        let kind = service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "widget")
            .unwrap();
        assert!(!kind.strict_properties);
    }

    #[tokio::test]
    async fn list_nodes_page_slices_and_counts() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        for index in 0..5 {
            service
                .create_node(&node(kind_id, &format!("p{index}"), ""), &[])
                .await
                .unwrap();
        }

        let page = service
            .list_nodes_page(
                &NodeFilter {
                    kind: Some("person".to_string()),
                    ..NodeFilter::default()
                },
                Some(2),
                Some(1),
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total, 5);
        assert_eq!(page.limit, 2);
        assert_eq!(page.offset, 1);

        let clamped = service
            .list_nodes_page(&NodeFilter::default(), Some(10_000), None)
            .await
            .unwrap();
        assert_eq!(
            clamped.limit,
            crate::config::PagingConfig::default().max_limit
        );

        let beyond = service
            .list_nodes_page(&NodeFilter::default(), Some(10), Some(100))
            .await
            .unwrap();
        assert!(beyond.items.is_empty());
        assert_eq!(beyond.total, 5);
    }

    #[tokio::test]
    async fn task_and_plan_writes_store_tags() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let mut task = new_task("tagged task");
        task.parent = Some(goal);
        task.tags = Some(vec!["Repo:X".to_string(), "phase:plan".to_string()]);
        let created = service.create_task(&task).await.unwrap();
        assert_eq!(
            service.node_tags(created.id).await.unwrap(),
            vec!["phase:plan", "repo:x"]
        );

        let mut update = new_task("tagged task");
        update.id = created.id;
        update.parent = Some(goal);
        update.description = "changed".to_string();
        service.create_task(&update).await.unwrap();
        assert_eq!(
            service.node_tags(created.id).await.unwrap(),
            vec!["phase:plan", "repo:x"]
        );

        service
            .update_task(
                created.id,
                &TaskPatch {
                    name: None,
                    description: None,
                    content: None,
                    properties: None,
                    tags: Some(vec!["other".to_string()]),
                },
            )
            .await
            .unwrap();
        assert_eq!(service.node_tags(created.id).await.unwrap(), vec!["other"]);

        let mut plan_task = plan_task("a", None, &[]);
        plan_task.tags = Some(vec!["Phase:Build".to_string()]);
        let mut draft = plan_draft(repo, vec![plan_task]);
        draft.goal.as_mut().unwrap().tags = Some(vec!["Goal:Ship".to_string()]);
        let plan = service.create_plan(&draft).await.unwrap();
        let plan_goal = plan.goal.as_ref().expect("goal").id;
        assert_eq!(
            service.node_tags(plan_goal).await.unwrap(),
            vec!["goal:ship"]
        );
        assert_eq!(
            service.node_tags(plan.tasks[0].node.id).await.unwrap(),
            vec!["phase:build"]
        );
    }

    #[tokio::test]
    async fn plan_upserts_leave_omitted_tags_and_clear_on_empty() {
        let service = service().await;
        let repo = seeded_repo(&service).await;

        let mut task = plan_task("a", None, &[]);
        task.id = 201;
        task.tags = Some(vec!["Phase:Build".to_string()]);
        let mut draft = plan_draft(repo, vec![task]);
        let goal = draft.goal.as_mut().expect("goal");
        goal.id = 200;
        goal.tags = Some(vec!["Goal:Ship".to_string()]);
        let first = service.create_plan(&draft).await.unwrap();
        let goal_id = first.goal.as_ref().expect("goal").id;
        let task_id = first.tasks[0].node.id;

        let mut omitted = plan_draft(repo, vec![plan_task("a", None, &[])]);
        let goal = omitted.goal.as_mut().expect("goal");
        goal.id = goal_id;
        goal.description = "changed".to_string();
        omitted.tasks[0].id = task_id;
        omitted.tasks[0].description = "changed".to_string();
        service.create_plan(&omitted).await.unwrap();
        assert_eq!(service.node_tags(goal_id).await.unwrap(), vec!["goal:ship"]);
        assert_eq!(
            service.node_tags(task_id).await.unwrap(),
            vec!["phase:build"]
        );

        let mut cleared = omitted.clone();
        cleared.goal.as_mut().expect("goal").tags = Some(Vec::new());
        cleared.tasks[0].tags = Some(Vec::new());
        service.create_plan(&cleared).await.unwrap();
        assert!(service.node_tags(goal_id).await.unwrap().is_empty());
        assert!(service.node_tags(task_id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn tag_and_property_upserts_bump_updated_at_only_on_change() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;

        let mut task = new_task("recency task");
        task.parent = Some(goal);
        task.tags = Some(vec!["a".to_string()]);
        task.properties = Some(serde_json::json!({"k": 1}));
        let created = service.create_task(&task).await.unwrap();
        let base = service.get_node(created.id).await.unwrap().updated_at;

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let mut same = new_task("recency task");
        same.id = created.id;
        same.parent = Some(goal);
        same.tags = Some(vec!["a".to_string()]);
        same.properties = Some(serde_json::json!({"k": 1}));
        service.create_task(&same).await.unwrap();
        assert_eq!(
            service.get_node(created.id).await.unwrap().updated_at,
            base,
            "an unchanged upsert must not bump updated_at"
        );

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let mut tags_only = same.clone();
        tags_only.tags = Some(vec!["b".to_string()]);
        service.create_task(&tags_only).await.unwrap();
        let after_tags = service.get_node(created.id).await.unwrap().updated_at;
        assert_ne!(after_tags, base, "a tag change must bump updated_at");
        assert_eq!(service.node_tags(created.id).await.unwrap(), vec!["b"]);

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let mut properties_only = same.clone();
        properties_only.tags = Some(vec!["b".to_string()]);
        properties_only.properties = Some(serde_json::json!({"k": 2}));
        service.create_task(&properties_only).await.unwrap();
        let after_properties = service.get_node(created.id).await.unwrap().updated_at;
        assert_ne!(
            after_properties, after_tags,
            "a property change must bump updated_at"
        );
    }

    #[tokio::test]
    async fn work_views_derive_status_from_implements_and_dependencies() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let change_kind = seeded_kind(&service, "change").await;
        let belongs_to = seeded_edge_type(&service, "belongs_to").await;
        let implements = seeded_edge_type(&service, "implements").await;

        let mut done = new_task("done task");
        done.parent = Some(goal);
        let done = service.create_task(&done).await.unwrap();
        let mut ready = new_task("ready task");
        ready.parent = Some(goal);
        ready.depends_on = vec![done.id];
        let ready = service.create_task(&ready).await.unwrap();
        let mut blocked = new_task("blocked task");
        blocked.parent = Some(goal);
        blocked.depends_on = vec![ready.id];
        let blocked = service.create_task(&blocked).await.unwrap();

        let change = service
            .upsert_node(&node(change_kind, "abc123", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: change.id,
                destination: done.id,
                edge_type_id: implements,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let pending = service
            .work_report(&WorkFilter::default(), false, None, None)
            .await
            .unwrap()
            .items;
        let statuses: Vec<(&str, WorkStatus)> = pending
            .iter()
            .map(|task| (task.name.as_str(), task.status))
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("ready task", WorkStatus::Pending),
                ("blocked task", WorkStatus::Blocked)
            ]
        );
        let ready_entry = pending.iter().find(|task| task.id == ready.id).unwrap();
        assert_eq!(ready_entry.goal_id, Some(goal));
        assert_eq!(ready_entry.depends_on.len(), 1);
        assert!(ready_entry.depends_on[0].done);
        assert!(ready_entry.implemented_by.is_empty());

        let next = service
            .work_report(&WorkFilter::default(), true, None, None)
            .await
            .unwrap()
            .items;
        assert_eq!(
            next.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![ready.id]
        );

        let (tree_goal, tree) = service.goal_tree(goal, None, None).await.unwrap();
        let tree = tree.items;
        assert_eq!(tree_goal.id, goal);
        assert_eq!(tree.len(), 3);
        let done_entry = tree.iter().find(|task| task.id == done.id).unwrap();
        assert_eq!(done_entry.status, WorkStatus::Done);
        assert_eq!(done_entry.implemented_by.len(), 1);
        assert_eq!(done_entry.implemented_by[0].name, "abc123");

        let other_goal = service
            .upsert_node(&node(goal_kind, "other goal", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: other_goal.id,
                destination: repo,
                edge_type_id: belongs_to,
                created_at: String::new(),
            })
            .await
            .unwrap();
        let mut other_task = new_task("other task");
        other_task.parent = Some(other_goal.id);
        let other_task = service.create_task(&other_task).await.unwrap();

        let in_goal = service
            .work_report(
                &WorkFilter {
                    goal: Some(goal),
                    repo: None,
                },
                false,
                None,
                None,
            )
            .await
            .unwrap()
            .items;
        assert!(in_goal.iter().all(|task| task.id != other_task.id));
        let in_repo = service
            .work_report(
                &WorkFilter {
                    goal: None,
                    repo: Some(repo),
                },
                false,
                None,
                None,
            )
            .await
            .unwrap()
            .items;
        assert!(in_repo.iter().any(|task| task.id == other_task.id));

        let page = service
            .work_report(&WorkFilter::default(), false, Some(1), Some(1))
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.limit, 1);
        assert_eq!(page.offset, 1);

        assert!(matches!(
            service.goal_tree(blocked.id, None, None).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service.goal_tree(999_999, None, None).await,
            Err(ServiceError::EntityNotFound)
        ));
        assert!(matches!(
            service
                .work_report(
                    &WorkFilter {
                        goal: Some(done.id),
                        repo: None,
                    },
                    false,
                    None,
                    None,
                )
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn creates_a_change_with_task_and_file_edges() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let file_kind = seeded_kind(&service, "file").await;
        service
            .upsert_node(&node(file_kind, "src/lib.rs", ""))
            .await
            .unwrap();
        service
            .upsert_node(&node(file_kind, "src/main.rs", ""))
            .await
            .unwrap();
        let mut task = new_task("change task");
        task.parent = Some(goal);
        let task = service.create_task(&task).await.unwrap();

        let draft = ChangeDraft {
            commit: "abc123".to_string(),
            title: "do work".to_string(),
            summary: "details".to_string(),
            tasks: vec![NodeSelector::Id(task.id), NodeSelector::Id(task.id)],
            files: vec!["src/lib.rs".to_string(), " src/main.rs ".to_string()],
            properties: None,
            tags: Some(vec!["Repo:X".to_string()]),
        };
        let change = service.create_change(&draft).await.unwrap();
        assert_eq!(change.name, "abc123");
        assert_eq!(change.description, "do work");
        assert_eq!(change.content, "details");
        assert_eq!(service.node_tags(change.id).await.unwrap(), vec!["repo:x"]);
        let outgoing = service
            .neighbors(change.id, NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(outgoing.len(), 3);
        let tree = service.goal_tree(goal, None, None).await.unwrap().1.items;
        assert_eq!(tree[0].status, WorkStatus::Done);
        assert_eq!(tree[0].implemented_by[0].name, "abc123");

        assert!(matches!(
            service.create_change(&draft).await,
            Err(ServiceError::Conflict(_))
        ));

        let mut bad_file = draft.clone();
        bad_file.commit = "def456".to_string();
        bad_file.files = vec!["missing.rs".to_string()];
        assert!(matches!(
            service.create_change(&bad_file).await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("missing.rs")
        ));

        let mut missing_task = draft.clone();
        missing_task.commit = "def456".to_string();
        missing_task.tasks = vec![NodeSelector::Id(999_999)];
        missing_task.files = vec![];
        assert!(matches!(
            service.create_change(&missing_task).await,
            Err(ServiceError::EntityNotFound)
        ));

        let mut wrong_kind = draft.clone();
        wrong_kind.commit = "def456".to_string();
        wrong_kind.tasks = vec![NodeSelector::Id(goal)];
        wrong_kind.files = vec![];
        assert!(matches!(
            service.create_change(&wrong_kind).await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("not a task")
        ));

        let mut empty = draft.clone();
        empty.commit = "def456".to_string();
        empty.tasks = vec![];
        empty.files = vec![];
        assert!(matches!(
            service.create_change(&empty).await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("at least one")
        ));

        let changes = service
            .list_nodes(&NodeFilter {
                kind: Some("change".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(changes.len(), 1);
    }

    #[tokio::test]
    async fn work_views_do_not_enumerate_paths_in_a_diamond_dag() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let task_kind = seeded_kind(&service, "task").await;
        let part_of = seeded_edge_type(&service, "part_of").await;

        let mut previous: Vec<i64> = Vec::new();
        for layer in 0..12 {
            let mut current = Vec::new();
            for lane in 0..2 {
                let created = service
                    .upsert_node(&node(task_kind, &format!("diamond {layer}-{lane}"), ""))
                    .await
                    .unwrap();
                if layer == 0 {
                    service
                        .add_edge(&Edge {
                            source: created.id,
                            destination: goal,
                            edge_type_id: part_of,
                            created_at: String::new(),
                        })
                        .await
                        .unwrap();
                } else {
                    for parent in &previous {
                        service
                            .add_edge(&Edge {
                                source: created.id,
                                destination: *parent,
                                edge_type_id: part_of,
                                created_at: String::new(),
                            })
                            .await
                            .unwrap();
                    }
                }
                current.push(created.id);
            }
            previous = current;
        }

        let started = std::time::Instant::now();
        let page = service
            .work_report(&WorkFilter::default(), false, None, None)
            .await
            .unwrap();
        assert_eq!(page.total, 24);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn work_views_terminate_on_a_part_of_cycle() {
        let service = service().await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![],
                edge_types: vec![PackEdgeType {
                    name: "part_of".to_string(),
                    description: None,
                    resolves: None,
                    single_outgoing: None,
                    acyclic: Some(false),
                    source_kinds: Some(vec!["task".to_string()]),
                    destination_kinds: Some(vec!["task".to_string(), "goal".to_string()]),
                }],
            })
            .await
            .unwrap();
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let task_kind = seeded_kind(&service, "task").await;
        let part_of = seeded_edge_type(&service, "part_of").await;
        let first = service
            .upsert_node(&node(task_kind, "cycle a", ""))
            .await
            .unwrap();
        let second = service
            .upsert_node(&node(task_kind, "cycle b", ""))
            .await
            .unwrap();
        for (source, destination) in [
            (first.id, goal),
            (second.id, first.id),
            (first.id, second.id),
        ] {
            service
                .add_edge(&Edge {
                    source,
                    destination,
                    edge_type_id: part_of,
                    created_at: String::new(),
                })
                .await
                .unwrap();
        }

        let page = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            service.work_report(&WorkFilter::default(), false, None, None),
        )
        .await
        .expect("work view must terminate on a cycle")
        .unwrap();
        assert_eq!(page.total, 2);
    }

    #[tokio::test]
    async fn changes_carry_properties_and_respect_strict_declarations() {
        let service = service().await;
        service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "change".to_string(),
                    description: None,
                    unique_name: None,
                    strict_properties: Some(true),
                    properties: Some(vec![property("author", "string", true)]),
                }],
                edge_types: vec![],
            })
            .await
            .unwrap();
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let mut task = new_task("property task");
        task.parent = Some(goal);
        let task = service.create_task(&task).await.unwrap();

        let draft = ChangeDraft {
            commit: "withprops".to_string(),
            title: "t".to_string(),
            summary: String::new(),
            tasks: vec![NodeSelector::Id(task.id)],
            files: vec![],
            properties: Some(serde_json::json!({"author": "ada"})),
            tags: None,
        };
        let change = service.create_change(&draft).await.unwrap();
        assert_eq!(
            service.node_properties(change.id).await.unwrap(),
            serde_json::json!({"author": "ada"})
        );

        let mut missing = draft.clone();
        missing.commit = "noprops".to_string();
        missing.properties = None;
        assert!(matches!(
            service.create_change(&missing).await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("author")
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_duplicate_changes_conflict_cleanly() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-change-race-{}-{unique}.sqlite",
            std::process::id()
        ));
        let service = file_service(&path).await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let mut task = new_task("race task");
        task.parent = Some(goal);
        let task = service.create_task(&task).await.unwrap();
        let draft = ChangeDraft {
            commit: "race123".to_string(),
            title: "race".to_string(),
            summary: String::new(),
            tasks: vec![NodeSelector::Id(task.id)],
            files: vec![],
            properties: None,
            tags: None,
        };

        let mut handles = Vec::new();
        for _ in 0..4 {
            let service = service.clone();
            let draft = draft.clone();
            handles.push(tokio::spawn(
                async move { service.create_change(&draft).await },
            ));
        }
        let mut created = 0;
        let mut conflicts = 0;
        for handle in handles {
            match handle.await.unwrap() {
                Ok(_) => created += 1,
                Err(ServiceError::Conflict(_)) => conflicts += 1,
                Err(error) => panic!("unexpected error: {error:?}"),
            }
        }
        assert_eq!((created, conflicts), (1, 3));

        drop(service);
        remove_sqlite(&path);
    }

    #[tokio::test]
    async fn nodes_by_ids_preserve_request_order_and_dedupe() {
        let service = service().await;
        let kind = seed_person(&service).await;
        let first = service
            .create_node(&node(kind, "first", ""), &["Math".to_string()])
            .await
            .unwrap()
            .0;
        let second = service
            .upsert_node(&node(kind, "second", ""))
            .await
            .unwrap();

        let nodes = service
            .nodes_by_ids(&[second.id, first.id, second.id, 999_999])
            .await
            .unwrap();
        assert_eq!(
            nodes.iter().map(|item| item.node.id).collect::<Vec<_>>(),
            vec![second.id, first.id]
        );
        assert!(nodes[0].tags.is_empty());
        assert_eq!(nodes[1].tags, vec!["math"]);
        assert_eq!(nodes[1].properties, serde_json::json!({}));

        let too_many: Vec<i64> = (0..201).collect();
        assert!(matches!(
            service.nodes_by_ids(&too_many).await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("at most 200")
        ));
    }

    #[tokio::test]
    async fn filters_nodes_by_exact_and_contains_name() {
        let service = service().await;
        let kind = seed_person(&service).await;
        service
            .create_node(&node(kind, "Ada Lovelace", ""), &[])
            .await
            .unwrap();
        service
            .create_node(&node(kind, "Ada Byron", ""), &[])
            .await
            .unwrap();
        service
            .create_node(&node(kind, "Grace Hopper", ""), &[])
            .await
            .unwrap();

        let exact = service
            .list_nodes(&NodeFilter {
                name: Some("Ada Lovelace".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].name, "Ada Lovelace");

        let contains = service
            .list_nodes(&NodeFilter {
                name_contains: Some("Ada".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(contains.len(), 2);

        let case_sensitive = service
            .list_nodes(&NodeFilter {
                name_contains: Some("ada".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(case_sensitive.is_empty());
    }

    #[tokio::test]
    async fn search_covers_node_names() {
        let service = service().await;
        let kind = seed_person(&service).await;
        let ada = service
            .create_node(&node(kind, "Ada Lovelace", ""), &[])
            .await
            .unwrap()
            .0;
        let page = service
            .search(Some("Lovelace"), &[], &Default::default(), None, None)
            .await
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].node.id, ada.id);
    }

    #[tokio::test]
    async fn resolves_named_selectors_and_rejects_ambiguity() {
        let service = service().await;
        let kind = service
            .create_node_kind(&NodeKind {
                id: 0,
                version: 0,
                name: "thing".to_string(),
                description: String::new(),
                unique_name: false,
                strict_properties: false,
                updated_at: String::new(),
            })
            .await
            .unwrap();
        let first = service
            .create_node(&node(kind.id, "duplicate", ""), &[])
            .await
            .unwrap()
            .0;
        assert_eq!(
            service
                .resolve_node_selector(&NodeSelector::Named {
                    kind: "thing".to_string(),
                    name: "duplicate".to_string(),
                })
                .await
                .unwrap(),
            first.id
        );
        service
            .create_node(&node(kind.id, "duplicate", ""), &[])
            .await
            .unwrap();
        assert!(matches!(
            service
                .resolve_node_selector(&NodeSelector::Named {
                    kind: "thing".to_string(),
                    name: "duplicate".to_string(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("ambiguous")
        ));
        assert!(matches!(
            service
                .resolve_node_selector(&NodeSelector::Named {
                    kind: "thing".to_string(),
                    name: "missing".to_string(),
                })
                .await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("no thing named")
        ));
    }

    #[tokio::test]
    async fn export_rejects_graphs_over_the_limit() {
        let (service, pool) = service_with_pool().await;
        let task_kind = seeded_kind(&service, "task").await;
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50001) \
             INSERT INTO node (kind_id, name, description, content) \
             SELECT ?, 'bulk export ' || i, '', '' FROM n",
        )
        .bind(task_kind)
        .execute(&pool)
        .await
        .unwrap();

        assert!(matches!(
            service.export_graph().await,
            Err(ServiceError::PayloadTooLarge(message)) if message.contains("50001")
        ));
    }

    #[tokio::test]
    async fn lists_edges_with_filters_and_paging() {
        let service = service().await;
        let kind = seed_person(&service).await;
        let first = service.upsert_node(&node(kind, "first", "")).await.unwrap();
        let second = service
            .upsert_node(&node(kind, "second", ""))
            .await
            .unwrap();
        let knows = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        let likes = service
            .create_edge_type(&edge_type("likes", ""))
            .await
            .unwrap();
        for (source, destination, edge_type_id) in [
            (first.id, second.id, knows.id),
            (second.id, first.id, knows.id),
            (first.id, second.id, likes.id),
        ] {
            service
                .add_edge(&Edge {
                    source,
                    destination,
                    edge_type_id,
                    created_at: String::new(),
                })
                .await
                .unwrap();
        }

        let all = service
            .list_edges(&EdgeFilter::default(), None, None)
            .await
            .unwrap();
        assert_eq!(all.total, 3);
        assert_eq!(all.items.len(), 3);

        let typed = service
            .list_edges(
                &EdgeFilter {
                    edge_type: Some("knows".to_string()),
                    source: None,
                    destination: None,
                },
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(typed.total, 2);

        let from_first = service
            .list_edges(
                &EdgeFilter {
                    edge_type: None,
                    source: Some(first.id),
                    destination: None,
                },
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(from_first.total, 2);

        let to_second = service
            .list_edges(
                &EdgeFilter {
                    edge_type: None,
                    source: None,
                    destination: Some(second.id),
                },
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(to_second.total, 2);

        let page = service
            .list_edges(&EdgeFilter::default(), Some(1), Some(1))
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.limit, 1);
        assert_eq!(page.offset, 1);

        assert!(matches!(
            service
                .list_edges(
                    &EdgeFilter {
                        edge_type: Some("unknown".to_string()),
                        source: None,
                        destination: None,
                    },
                    None,
                    None,
                )
                .await,
            Err(ServiceError::UnprocessableEntity(message)) if message.contains("not defined")
        ));
    }

    #[tokio::test]
    async fn exports_a_compact_graph_snapshot() {
        let service = service().await;
        let kind = seed_person(&service).await;
        let first = service
            .create_node(&node(kind, "first", ""), &["Math".to_string()])
            .await
            .unwrap()
            .0;
        let second = service
            .upsert_node(&node(kind, "second", ""))
            .await
            .unwrap();
        let knows = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: first.id,
                destination: second.id,
                edge_type_id: knows.id,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let export = service.export_graph().await.unwrap();
        assert_eq!(export.nodes.len(), 2);
        assert!(
            export
                .nodes
                .iter()
                .any(|node| node.name == "first" && node.tags == vec!["math"])
        );
        assert_eq!(export.edges.len(), 1);
        assert_eq!(export.edges[0].source, first.id);
        assert_eq!(export.edges[0].destination, second.id);
    }
}
