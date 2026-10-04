use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::{
    config::SearchConfig,
    model::{
        Edge, EdgeType, Neighbor, NeighborDirection, Node, NodeFilter, NodeKind, NodeRef,
        PendingEdge, Plan, PlanDraft, PlanTask, PlanTaskDraft, ReachFilter, SearchHit, SearchScope,
        SegmentMatches, TaskDraft, TaskPatch,
    },
    repository::graph::GraphRepository,
};

use super::error::ServiceError;

const DEFAULT_DESCEND: &str = "part_of";
const DESCEND_NONE: &str = "none";

#[derive(Clone)]
pub struct GraphService {
    repository: Arc<dyn GraphRepository>,
    search_config: SearchConfig,
}

impl GraphService {
    pub fn new(repository: Arc<dyn GraphRepository>, search_config: SearchConfig) -> Self {
        Self {
            repository,
            search_config,
        }
    }

    pub async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, ServiceError> {
        self.repository
            .create_node_kind(node_kind)
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
        self.repository
            .create_edge_type(edge_type)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_edge_types(&self) -> Result<Vec<EdgeType>, ServiceError> {
        self.repository
            .list_edge_types()
            .await
            .map_err(ServiceError::from)
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
            scope: filter.scope,
            reach: self.normalize_reach(&filter.reach).await?,
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
            .add_edge(edge)
            .await
            .map_err(ServiceError::from)
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
                "provide at least one of name, description or content".to_string(),
            ));
        }
        let node = self.get_node(task_id).await?;
        let task_kind = self
            .repository
            .list_node_kinds()
            .await?
            .into_iter()
            .find(|kind| kind.name == "task")
            .ok_or_else(|| {
                ServiceError::UnprocessableEntity("node kind 'task' is not defined".to_string())
            })?
            .id;
        if node.kind_id != task_kind {
            return Err(ServiceError::UnprocessableEntity(format!(
                "node {task_id} is not a task"
            )));
        }
        let updated = Node {
            id: node.id,
            kind_id: node.kind_id,
            name: patch.name.clone().unwrap_or(node.name),
            description: patch.description.clone().unwrap_or(node.description),
            content: patch.content.clone().unwrap_or(node.content),
            updated_at: String::new(),
        };
        self.repository
            .upsert_node(&updated)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn create_task(&self, draft: &TaskDraft) -> Result<Node, ServiceError> {
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
        let task_kind = self
            .repository
            .list_node_kinds()
            .await?
            .into_iter()
            .find(|kind| kind.name == "task")
            .ok_or_else(|| {
                ServiceError::UnprocessableEntity("node kind 'task' is not defined".to_string())
            })?
            .id;
        let edge_types = self.repository.list_edge_types().await?;
        let part_of = edge_type_ref(&edge_types, "part_of")?;
        let depends_on = edge_type_ref(&edge_types, "depends_on")?;
        let affects = edge_type_ref(&edge_types, "affects")?;

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
        for dependency in &draft.depends_on {
            require_kind(
                &kinds,
                *dependency,
                &depends_on.destination_kind_list(),
                "depends_on target",
            )?;
        }
        for target in &draft.affects {
            require_kind(
                &kinds,
                *target,
                &affects.destination_kind_list(),
                "affects target",
            )?;
        }
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

        let mut created = self
            .repository
            .insert_nodes_with_edges(std::slice::from_ref(&node), &edges)
            .await?;
        created.pop().ok_or_else(|| {
            ServiceError::InternalError("task creation returned no node".to_string())
        })
    }

    pub async fn create_plan(&self, draft: &PlanDraft) -> Result<Plan, ServiceError> {
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
                "a plan requires an anchor: provide anchor to attach the goal to a repo"
                    .to_string(),
            ));
        }

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

        let node_kinds = self.repository.list_node_kinds().await?;
        let task_kind = node_kinds
            .iter()
            .find(|kind| kind.name == "task")
            .map(|kind| kind.id)
            .ok_or_else(|| {
                ServiceError::UnprocessableEntity("node kind 'task' is not defined".to_string())
            })?;
        let goal_kind = if draft.goal.is_some() {
            Some(
                node_kinds
                    .iter()
                    .find(|kind| kind.name == "goal")
                    .map(|kind| kind.id)
                    .ok_or_else(|| {
                        ServiceError::UnprocessableEntity(
                            "node kind 'goal' is not defined".to_string(),
                        )
                    })?,
            )
        } else {
            None
        };
        let edge_types = self.repository.list_edge_types().await?;
        let part_of = edge_type_ref(&edge_types, "part_of")?;
        let depends_on = edge_type_ref(&edge_types, "depends_on")?;
        let affects = edge_type_ref(&edge_types, "affects")?;

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
        for task in &draft.tasks {
            for target in &task.affects {
                require_kind(
                    &kinds,
                    *target,
                    &affects.destination_kind_list(),
                    &format!("plan task '{}' affects", task.key),
                )?;
            }
        }

        let anchor_edge = match &draft.anchor {
            Some(anchor) => {
                let edge_type = edge_type_ref(&edge_types, &anchor.edge_type)?;
                let source_kinds = edge_type.source_kind_list();
                if !source_kinds.is_empty() && !source_kinds.contains(&"goal") {
                    return Err(ServiceError::UnprocessableEntity(format!(
                        "edge type '{}' does not accept a goal source",
                        edge_type.name
                    )));
                }
                require_kind(
                    &kinds,
                    anchor.node_id,
                    &edge_type.destination_kind_list(),
                    &format!("plan anchor edge '{}' destination", edge_type.name),
                )?;
                if let Some(goal_id) = draft.goal_id {
                    self.require_single_outgoing(goal_id, anchor.node_id, edge_type)
                        .await?;
                }
                Some((edge_type.id, anchor.node_id))
            }
            None => None,
        };

        let mut key_index: HashMap<&str, usize> = HashMap::new();
        let offset = if draft.goal.is_some() { 1 } else { 0 };
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

        assert_acyclic_parents(&draft.tasks)?;
        assert_acyclic_dependencies(&draft.tasks)?;

        let mut nodes = Vec::with_capacity(offset + draft.tasks.len());
        if let (Some(goal), Some(goal_kind)) = (&draft.goal, goal_kind) {
            nodes.push(Node {
                id: goal.id,
                kind_id: goal_kind,
                name: goal.name.clone(),
                description: goal.description.clone(),
                content: goal.content.clone(),
                updated_at: String::new(),
            });
        }
        for task in &draft.tasks {
            nodes.push(Node {
                id: task.id,
                kind_id: task_kind,
                name: task.name.clone(),
                description: task.description.clone(),
                content: task.content.clone(),
                updated_at: String::new(),
            });
        }

        let root = match draft.goal_id {
            Some(goal_id) => NodeRef::Existing(goal_id),
            None => NodeRef::New(0),
        };

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
                Some(parent) => NodeRef::New(key_index[parent.as_str()]),
                None => root,
            };
            edges.push(PendingEdge {
                source: task_ref,
                destination: parent,
                edge_type_id: part_of.id,
            });
            for dependency in &task.depends_on {
                edges.push(PendingEdge {
                    source: task_ref,
                    destination: NodeRef::New(key_index[dependency.as_str()]),
                    edge_type_id: depends_on.id,
                });
            }
            for target in &task.affects {
                edges.push(PendingEdge {
                    source: task_ref,
                    destination: NodeRef::Existing(*target),
                    edge_type_id: affects.id,
                });
            }
        }

        let created = self
            .repository
            .insert_nodes_with_edges(&nodes, &edges)
            .await?;
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
        Ok(Plan { goal, tasks })
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

    pub async fn search(
        &self,
        query: &str,
        scope: SearchScope,
        reach: &ReachFilter,
    ) -> Result<Vec<SegmentMatches>, ServiceError> {
        let reach = self.normalize_reach(reach).await?;
        let mut segments = Vec::new();
        for segment in split_segments(query, &self.search_config.split_characters()) {
            let score = self.score_segment(&segment, scope, &reach).await?;
            if score.hits.is_empty() && score.excluded_resolved == 0 {
                continue;
            }
            let total_matches = score.hits.len();
            let matches = score
                .hits
                .into_iter()
                .take(self.search_config.per_segment_limit)
                .collect();
            segments.push(SegmentMatches {
                segment,
                matches,
                total_matches,
                excluded_resolved: score.excluded_resolved,
            });
        }
        Ok(segments)
    }

    pub async fn search_segment(
        &self,
        segment: &str,
        scope: SearchScope,
        reach: &ReachFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, ServiceError> {
        let reach = self.normalize_reach(reach).await?;
        let score = self.score_segment(segment, scope, &reach).await?;
        Ok(score.hits.into_iter().skip(offset).take(limit).collect())
    }

    pub fn summary_chars(&self) -> usize {
        self.search_config.summary_chars
    }

    pub fn default_segment_limit(&self) -> usize {
        self.search_config.per_segment_limit
    }

    async fn score_segment(
        &self,
        segment: &str,
        scope: SearchScope,
        reach: &ReachFilter,
    ) -> Result<SegmentScore, ServiceError> {
        let considered = self.considered_terms(segment).await?;
        if considered.is_empty() {
            return Ok(SegmentScore::default());
        }
        let mut seen_tokens: Vec<&str> = Vec::new();
        let mut denominator = 0.0;
        for considered_term in &considered {
            if !seen_tokens.contains(&considered_term.token.as_str()) {
                seen_tokens.push(considered_term.token.as_str());
                denominator += considered_term.weight;
            }
        }
        let terms: Vec<&str> = considered
            .iter()
            .map(|considered_term| considered_term.term.as_str())
            .collect();
        let fts_query = build_fts_query(&terms);
        let candidates = self
            .repository
            .search_descriptions(&fts_query, reach)
            .await?;

        let mut hits = Vec::new();
        let mut excluded_resolved = 0;
        for (node, resolved) in candidates {
            let tokens: HashSet<String> = tokenize(&node.description).into_iter().collect();
            let mut matched: Vec<String> = Vec::new();
            let mut matched_tokens: Vec<&str> = Vec::new();
            let mut mass = 0.0;
            for considered_term in &considered {
                if !tokens.contains(&considered_term.term) {
                    continue;
                }
                matched.push(considered_term.term.clone());
                if !matched_tokens.contains(&considered_term.token.as_str()) {
                    matched_tokens.push(considered_term.token.as_str());
                    mass += considered_term.weight;
                }
            }
            let confidence = if denominator > 0.0 {
                mass / denominator
            } else {
                0.0
            };
            if matched_tokens.len() < self.search_config.min_matched_terms {
                continue;
            }
            if mass < self.search_config.min_matched_idf {
                continue;
            }
            if confidence < self.search_config.confidence_threshold {
                continue;
            }
            if scope == SearchScope::Active && resolved {
                excluded_resolved += 1;
                continue;
            }
            if scope == SearchScope::Resolved && !resolved {
                continue;
            }
            hits.push(SearchHit {
                relations: self.repository.relation_summaries(node.id).await?,
                node,
                confidence,
                matched_terms: matched,
                resolved,
            });
        }
        hits.sort_by(|left, right| {
            right
                .confidence
                .partial_cmp(&left.confidence)
                .unwrap_or(Ordering::Equal)
                .then(left.node.id.cmp(&right.node.id))
        });
        Ok(SegmentScore {
            hits,
            excluded_resolved,
        })
    }

    async fn considered_terms(&self, segment: &str) -> Result<Vec<ConsideredTerm>, ServiceError> {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut assigned: HashSet<String> = HashSet::new();
        for token in tokenize(segment) {
            if !seen.insert(token.clone()) {
                continue;
            }
            let terms: Vec<String> = self
                .resolve_token(&token)
                .await?
                .into_iter()
                .filter(|term| assigned.insert(term.clone()))
                .collect();
            if !terms.is_empty() {
                groups.push((token, terms));
            }
        }
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        let node_count = self.repository.count_nodes().await? as f64;
        let mut weighted = Vec::new();
        for (token, terms) in groups {
            for term in terms {
                let frequency = self.repository.term_document_frequency(&term).await? as f64;
                weighted.push(ConsideredTerm {
                    token: token.clone(),
                    term,
                    weight: inverse_document_frequency(node_count, frequency),
                });
            }
        }
        weighted.sort_by(|left, right| {
            right
                .weight
                .partial_cmp(&left.weight)
                .unwrap_or(Ordering::Equal)
                .then(left.term.cmp(&right.term))
        });
        weighted.truncate(self.search_config.max_terms);
        Ok(weighted)
    }

    async fn resolve_token(&self, token: &str) -> Result<Vec<String>, ServiceError> {
        let min_length = self.search_config.prefix_min_length;
        let characters: Vec<char> = token.chars().collect();
        let mut terms: BTreeSet<String> = BTreeSet::new();
        if self.repository.term_exists(token).await? {
            terms.insert(token.to_string());
        }
        if characters.len() >= min_length {
            for term in self.repository.terms_with_prefix(token).await? {
                terms.insert(term);
            }
        }
        for length in min_length..characters.len() {
            let candidate: String = characters[..length].iter().collect();
            if self.repository.term_exists(&candidate).await? {
                terms.insert(candidate);
            }
        }
        Ok(terms.into_iter().collect())
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

struct ConsideredTerm {
    token: String,
    term: String,
    weight: f64,
}

#[derive(Default)]
struct SegmentScore {
    hits: Vec<SearchHit>,
    excluded_resolved: usize,
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

fn inverse_document_frequency(node_count: f64, document_frequency: f64) -> f64 {
    (1.0 + node_count / document_frequency).ln()
}

fn split_segments(text: &str, separators: &[char]) -> Vec<String> {
    text.split(|character| separators.contains(&character))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn build_fts_query(terms: &[&str]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<String>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use crate::{
        database::{create_sqlite_pool, migrate},
        model::{PlanAnchorDraft, PlanGoalDraft, RelationDirection},
        repository::graph::SqliteGraphRepository,
    };

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
        GraphService::new(
            Arc::new(SqliteGraphRepository::new(pool)),
            test_search_config(),
        )
    }

    fn test_search_config() -> crate::config::SearchConfig {
        crate::config::SearchConfig {
            min_matched_idf: 0.0,
            ..crate::config::SearchConfig::default()
        }
    }

    fn node_kind(name: &str) -> NodeKind {
        NodeKind {
            id: 0,
            name: name.to_string(),
            description: String::new(),
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
                .search("ada lovelace", SearchScope::All, &ReachFilter::default())
                .await
                .unwrap()
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
                .search("ada lovelace", SearchScope::All, &ReachFilter::default())
                .await
                .unwrap()
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

    #[tokio::test]
    async fn search_matches_and_reflects_updates() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&described_node(
                kind_id,
                "Ada Lovelace",
                "pioneer of computing",
            ))
            .await
            .unwrap();
        let segments = service
            .search(
                "pioneer computing",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].matches.len(), 1);
        assert_eq!(segments[0].matches[0].node.id, ada.id);
        assert!(segments[0].matches[0].confidence >= 0.99);

        let mut changed = ada.clone();
        changed.description = "mathematician and logician".to_string();
        service.upsert_node(&changed).await.unwrap();
        assert!(
            service
                .search(
                    "pioneer computing",
                    SearchScope::All,
                    &ReachFilter::default()
                )
                .await
                .unwrap()
                .is_empty()
        );
        let segments = service
            .search(
                "mathematician logician",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].matches[0].node.id, ada.id);
    }

    #[tokio::test]
    async fn gates_out_single_term_overlap() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        service
            .upsert_node(&described_node(kind_id, "Ledger", "owned debt record"))
            .await
            .unwrap();
        assert!(
            service
                .search("debt repayment", SearchScope::All, &ReachFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn prefix_variants_do_not_deflate_confidence() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        for (name, description) in [
            ("schema", "add refresh token table with rotation"),
            ("endpoint", "post auth refresh issues rotated token pairs"),
            ("client", "cli retries once after refreshing"),
            ("session", "users stay signed in across access token expiry"),
        ] {
            service
                .upsert_node(&described_node(kind_id, name, description))
                .await
                .unwrap();
        }
        let segments = service
            .search(
                "refresh rotation",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].matches.len(), 1);
        assert_eq!(segments[0].matches[0].node.name, "schema");
        let confidence = segments[0].matches[0].confidence;
        assert!(confidence > 0.8);
        assert!(confidence < 0.9);
    }

    #[tokio::test]
    async fn prefix_variants_of_one_token_count_as_one_match() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        service
            .upsert_node(&described_node(kind_id, "Ledger", "refresh refreshing"))
            .await
            .unwrap();
        assert!(
            service
                .search("refresh", SearchScope::All, &ReachFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn splits_multi_topic_prompt() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let composer = service
            .upsert_node(&described_node(kind_id, "Composer", "musical composition"))
            .await
            .unwrap();
        let database = service
            .upsert_node(&described_node(kind_id, "Database", "query storage engine"))
            .await
            .unwrap();
        let segments = service
            .search(
                "musical composition? query storage engine",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].matches[0].node.id, composer.id);
        assert_eq!(segments[1].matches[0].node.id, database.id);
    }

    #[tokio::test]
    async fn search_hit_includes_topology_probe() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&described_node(kind_id, "Ada", "ada lovelace"))
            .await
            .unwrap();
        let bob = service
            .upsert_node(&described_node(kind_id, "Bob", "ada lovelace"))
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
        let segments = service
            .search("ada lovelace", SearchScope::All, &ReachFilter::default())
            .await
            .unwrap();
        let ada_hit = segments[0]
            .matches
            .iter()
            .find(|hit| hit.node.id == ada.id)
            .unwrap();
        assert_eq!(ada_hit.relations.len(), 1);
        assert_eq!(ada_hit.relations[0].direction, RelationDirection::Outgoing);
        assert_eq!(ada_hit.relations[0].edge_type, "knows");
        assert_eq!(ada_hit.relations[0].count, 1);
        let bob_hit = segments[0]
            .matches
            .iter()
            .find(|hit| hit.node.id == bob.id)
            .unwrap();
        assert_eq!(bob_hit.relations[0].direction, RelationDirection::Ingoing);
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
        }
    }

    fn plan_draft(repo: i64, tasks: Vec<PlanTaskDraft>) -> PlanDraft {
        PlanDraft {
            goal: Some(PlanGoalDraft {
                id: 0,
                name: "ship feature".to_string(),
                description: String::new(),
                content: String::new(),
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

        let segments = service
            .search(
                "recalibrated widget",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(segments[0].matches[0].node.id, created.id);
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

    #[tokio::test]
    async fn scopes_search_by_resolving_edges() {
        let service = service().await;
        let repo = seeded_repo(&service).await;
        let goal = anchored_goal(&service, repo).await;
        let change_kind = seeded_kind(&service, "change").await;
        let implements = seeded_edge_type(&service, "implements").await;

        let mut draft = new_task("audited");
        draft.description = "recalibrated widget".to_string();
        draft.parent = Some(goal);
        let task = service.create_task(&draft).await.unwrap();
        let change = service
            .upsert_node(&Node {
                id: 0,
                kind_id: change_kind,
                name: "commit".to_string(),
                description: String::new(),
                content: String::new(),
                updated_at: String::new(),
            })
            .await
            .unwrap();

        let active = service
            .search(
                "recalibrated widget",
                SearchScope::Active,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].matches[0].node.id, task.id);
        assert!(!active[0].matches[0].resolved);
        assert_eq!(active[0].excluded_resolved, 0);

        service
            .add_edge(&Edge {
                source: change.id,
                destination: task.id,
                edge_type_id: implements,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let active = service
            .search(
                "recalibrated widget",
                SearchScope::Active,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert!(active[0].matches.is_empty());
        assert_eq!(active[0].total_matches, 0);
        assert_eq!(active[0].excluded_resolved, 1);

        let all = service
            .search(
                "recalibrated widget",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(all[0].matches.len(), 1);
        assert!(all[0].matches[0].resolved);
        assert_eq!(all[0].excluded_resolved, 0);

        let resolved = service
            .search(
                "recalibrated widget",
                SearchScope::Resolved,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(resolved[0].matches.len(), 1);
        assert_eq!(resolved[0].matches[0].node.id, task.id);
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
                kind: Some("task".to_string()),
                scope: SearchScope::All,
                reach: ReachFilter {
                    within: Some(repo),
                    via: Some("belongs_to".to_string()),
                    descend: None,
                },
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
                kind: Some("goal".to_string()),
                scope: SearchScope::All,
                reach: ReachFilter {
                    within: Some(repo),
                    via: Some("belongs_to".to_string()),
                    descend: Some("none".to_string()),
                },
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
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: None,
                        via: Some("belongs_to".to_string()),
                        descend: None,
                    },
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: Some(repo),
                        via: Some("nope".to_string()),
                        descend: None,
                    },
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: Some(repo),
                        via: Some(DESCEND_NONE.to_string()),
                        descend: None,
                    },
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    kind: None,
                    scope: SearchScope::All,
                    reach: ReachFilter {
                        within: Some(9999),
                        via: None,
                        descend: None,
                    },
                })
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .list_nodes(&NodeFilter {
                    kind: Some("nope".to_string()),
                    scope: SearchScope::All,
                    reach: ReachFilter::default(),
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
            kind: Some("task".to_string()),
            scope,
            reach: ReachFilter {
                within: Some(repo),
                via: Some("belongs_to".to_string()),
                descend: None,
            },
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
    async fn search_respects_reach_filter() {
        let service = service().await;
        let repo_kind = seeded_kind(&service, "repo").await;
        let goal_kind = seeded_kind(&service, "goal").await;
        let task_kind = seeded_kind(&service, "task").await;
        let belongs_to = seeded_edge_type(&service, "belongs_to").await;
        let part_of = seeded_edge_type(&service, "part_of").await;

        let repo = service
            .upsert_node(&node(repo_kind, "acme/widget", ""))
            .await
            .unwrap();
        let goal = service
            .upsert_node(&node(goal_kind, "ship widget", ""))
            .await
            .unwrap();
        let scoped = service
            .upsert_node(&described_node(
                task_kind,
                "build widget",
                "recalibrated widget",
            ))
            .await
            .unwrap();
        let loose = service
            .upsert_node(&described_node(
                task_kind,
                "build gadget",
                "recalibrated widget",
            ))
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
                source: scoped.id,
                destination: goal.id,
                edge_type_id: part_of,
                created_at: String::new(),
            })
            .await
            .unwrap();

        let all = service
            .search(
                "recalibrated widget",
                SearchScope::All,
                &ReachFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(all[0].total_matches, 2);

        let reach = ReachFilter {
            within: Some(repo.id),
            via: Some("belongs_to".to_string()),
            descend: None,
        };
        let filtered = service
            .search("recalibrated widget", SearchScope::All, &reach)
            .await
            .unwrap();
        assert_eq!(filtered[0].total_matches, 1);
        assert_eq!(filtered[0].matches[0].node.id, scoped.id);
        assert_ne!(filtered[0].matches[0].node.id, loose.id);
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

        let draft = |anchor: PlanAnchorDraft| PlanDraft {
            goal: Some(PlanGoalDraft {
                id: 0,
                name: "ship widget".to_string(),
                description: String::new(),
                content: String::new(),
            }),
            goal_id: None,
            anchor: Some(anchor),
            tasks: vec![plan_task("a", None, &[])],
        };

        let plan = service
            .create_plan(&draft(PlanAnchorDraft {
                node_id: repo.id,
                edge_type: "belongs_to".to_string(),
            }))
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
                .create_plan(&draft(PlanAnchorDraft {
                    node_id: repo.id,
                    edge_type: "contains".to_string(),
                }))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .create_plan(&draft(PlanAnchorDraft {
                    node_id: 9999,
                    edge_type: "belongs_to".to_string(),
                }))
                .await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
        assert!(matches!(
            service
                .create_plan(&draft(PlanAnchorDraft {
                    node_id: repo.id,
                    edge_type: "nope".to_string(),
                }))
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
                kind: Some("goal".to_string()),
                scope: SearchScope::All,
                reach: ReachFilter {
                    within: Some(other_repo),
                    via: Some("belongs_to".to_string()),
                    descend: Some("none".to_string()),
                },
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
    async fn anchored_check_terminates_on_a_part_of_cycle() {
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
        for (source, destination) in [(first.id, second.id), (second.id, first.id)] {
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

        let mut draft = new_task("child");
        draft.parent = Some(first.id);
        let error = service.create_task(&draft).await.unwrap_err();
        assert!(rejection(error).contains("is not attached to a repo"));
    }
}
