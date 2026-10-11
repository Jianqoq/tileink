use super::model::*;
use super::prelude::*;
use super::transaction::{Mutation, RetainedSceneTransaction, UndoMutation};
use super::validation::validate_size;

pub(crate) const JOURNAL_CAPACITY: usize = 256;
static NEXT_SCENE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct JournalEntry {
    version: SceneVersion,
    changes: SceneChangeSet,
}

pub struct RetainedScene {
    pub(crate) id: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale: f32,
    pub(crate) root: RetainedNodeId,
    pub(crate) version: SceneVersion,
    pub(crate) nodes: HashMap<RetainedNodeId, SceneNode>,
    journal: VecDeque<JournalEntry>,
    pub(crate) next_node_instance: u64,
}

impl RetainedScene {
    pub fn new(
        width: u32,
        height: u32,
        scale: f32,
        root: RetainedNodeId,
    ) -> Result<Self, RetainedSceneError> {
        validate_size(width, height, scale)?;
        let mut nodes = HashMap::default();
        nodes.insert(root, SceneNode::group(None, 1));
        Ok(Self {
            id: NEXT_SCENE_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            scale,
            root,
            version: SceneVersion::INITIAL,
            nodes,
            journal: VecDeque::new(),
            next_node_instance: 2,
        })
    }

    pub fn version(&self) -> SceneVersion {
        self.version
    }

    pub fn root(&self) -> RetainedNodeId {
        self.root
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (
            ((self.width as f64) * f64::from(self.scale)).ceil() as u32,
            ((self.height as f64) * f64::from(self.scale)).ceil() as u32,
        )
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub fn transaction(&mut self) -> RetainedSceneTransaction<'_> {
        RetainedSceneTransaction {
            scene: self,
            mutations: Vec::new(),
        }
    }

    pub(crate) fn changes_since(&self, version: SceneVersion) -> Option<SceneChangeSet> {
        if version == self.version {
            return Some(SceneChangeSet::default());
        }
        let first = self.journal.front()?.version.get();
        if version.get().saturating_add(1) < first {
            return None;
        }
        let mut changes = SceneChangeSet::default();
        for entry in self.journal.iter().filter(|entry| entry.version > version) {
            changes.merge(&entry.changes);
        }
        Some(changes)
    }

    pub(crate) fn commit_mutations(
        &mut self,
        mutations: Vec<Mutation>,
    ) -> Result<SceneVersion, RetainedSceneError> {
        let mut changes = SceneChangeSet::default();
        let mut undo = Vec::with_capacity(mutations.len());
        for mutation in mutations {
            match self.apply_with_undo(mutation, &mut changes) {
                Ok(entry) => undo.push(entry),
                Err(error) => {
                    for entry in undo.into_iter().rev() {
                        self.undo(entry);
                    }
                    return Err(error);
                }
            }
        }
        if changes == SceneChangeSet::default() {
            return Ok(self.version);
        }
        self.version = SceneVersion(self.version.0.wrapping_add(1));
        self.journal.push_back(JournalEntry {
            version: self.version,
            changes,
        });
        while self.journal.len() > JOURNAL_CAPACITY {
            self.journal.pop_front();
        }
        Ok(self.version)
    }

    fn undo(&mut self, mutation: UndoMutation) {
        match mutation {
            UndoMutation::None => {}
            UndoMutation::Insert {
                parent,
                id,
                insertion,
            } => {
                self.undo_child_insertion(parent, id, insertion);
                self.nodes.remove(&id).expect("undo inserted node exists");
            }
            UndoMutation::NodeValue {
                id,
                kind,
                generation,
            } => {
                let node = self.nodes.get_mut(&id).expect("undo node exists");
                node.kind = kind;
                node.generation = generation;
            }
            UndoMutation::Reparent {
                id,
                old_parent,
                old_key,
                new_parent,
                new_insertion,
            } => {
                self.undo_child_insertion(new_parent, id, new_insertion);
                self.insert_child_at(old_parent, old_key, id);
                self.nodes.get_mut(&id).unwrap().parent = Some(old_parent);
            }
            UndoMutation::Remove {
                parent,
                parent_key,
                nodes,
            } => {
                let root = nodes[0].0;
                self.nodes.extend(nodes);
                self.insert_child_at(parent, parent_key, root);
            }
            UndoMutation::Resize((width, height, scale)) => {
                (self.width, self.height, self.scale) = (width, height, scale);
            }
        }
    }

    pub(super) fn insert_child(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> Result<ChildInsertion, RetainedSceneError> {
        self.nodes
            .get_mut(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children_mut(parent)?
            .insert_before(id, before)
    }

    fn undo_child_insertion(
        &mut self,
        parent: RetainedParent,
        id: RetainedNodeId,
        insertion: ChildInsertion,
    ) {
        if let Some(before_rebalance) = insertion.before_rebalance {
            *self
                .nodes
                .get_mut(&parent.node)
                .expect("undo parent exists")
                .children_mut(parent)
                .expect("undo branch is valid") = before_rebalance;
        } else {
            let removed = self.remove_child(parent, id);
            debug_assert_eq!(removed, Ok(insertion.key));
        }
    }

    pub(super) fn validate_child_insert(
        &self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> Result<(), RetainedSceneError> {
        let children = self
            .nodes
            .get(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children(parent)?;
        if let Some(before) = before
            && (before == id || children.key_of(before).is_none())
        {
            return Err(RetainedSceneError::InvalidSibling(before));
        }
        Ok(())
    }

    fn insert_child_at(&mut self, parent: RetainedParent, key: u128, id: RetainedNodeId) {
        self.nodes
            .get_mut(&parent.node)
            .expect("undo parent exists")
            .children_mut(parent)
            .expect("undo branch is valid")
            .insert_at(key, id);
    }

    pub(super) fn remove_child(
        &mut self,
        parent: RetainedParent,
        id: RetainedNodeId,
    ) -> Result<u128, RetainedSceneError> {
        self.nodes
            .get_mut(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children_mut(parent)?
            .remove(id)
            .ok_or(RetainedSceneError::MissingNode(id))
    }

    pub(super) fn ensure_no_cycle(
        &self,
        id: RetainedNodeId,
        mut parent: RetainedNodeId,
    ) -> Result<(), RetainedSceneError> {
        loop {
            if parent == id {
                return Err(RetainedSceneError::Cycle(id));
            }
            let node = self
                .nodes
                .get(&parent)
                .ok_or(RetainedSceneError::MissingNode(parent))?;
            let Some(next) = node.parent else {
                return Ok(());
            };
            parent = next.node;
        }
    }

    pub(crate) fn collect_subtree(&self, id: RetainedNodeId, out: &mut Vec<RetainedNodeId>) {
        out.push(id);
        let node = &self.nodes[&id];
        for child in node.content.values().chain(node.mask.values()) {
            self.collect_subtree(*child, out);
        }
    }
}
