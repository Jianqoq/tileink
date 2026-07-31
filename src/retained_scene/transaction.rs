use super::model::*;
use super::prelude::*;
use super::scene::RetainedScene;

pub(crate) enum Mutation {
    Insert {
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        kind: NodeKind,
    },
    ReplaceScene {
        id: RetainedNodeId,
        canvas: Rc<Canvas>,
    },
    SetTransform {
        id: RetainedNodeId,
        transform: Affine,
        translation_damage: Option<Rect>,
    },
    UpdateLayer {
        id: RetainedNodeId,
        layer: RetainedLayerDescriptor,
    },
    Reparent {
        id: RetainedNodeId,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
    },
    MoveBefore {
        id: RetainedNodeId,
        sibling: RetainedNodeId,
    },
    Remove {
        id: RetainedNodeId,
    },
    Resize {
        width: u32,
        height: u32,
        scale: f32,
    },
    InvalidateRect(Rect),
    InvalidateAll,
}

pub(crate) enum UndoMutation {
    None,
    Insert {
        parent: RetainedParent,
        id: RetainedNodeId,
        insertion: ChildInsertion,
    },
    NodeValue {
        id: RetainedNodeId,
        kind: NodeKind,
        generation: u64,
    },
    Reparent {
        id: RetainedNodeId,
        old_parent: RetainedParent,
        old_key: u128,
        new_parent: RetainedParent,
        new_insertion: ChildInsertion,
    },
    Remove {
        parent: RetainedParent,
        parent_key: u128,
        nodes: Vec<(RetainedNodeId, SceneNode)>,
    },
    Resize((u32, u32, f32)),
}

pub struct RetainedSceneTransaction<'a> {
    pub(crate) scene: &'a mut RetainedScene,
    pub(crate) mutations: Vec<Mutation>,
}

impl RetainedSceneTransaction<'_> {
    pub fn insert_scene(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        canvas: Rc<Canvas>,
        transform: Affine,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Scene {
                canvas,
                transform,
                translation_damage: None,
            },
        });
        self
    }

    /// Inserts a retained scene whose translated output is constrained to a fixed logical region.
    ///
    /// Subsequent [`Self::set_bounded_translation`] calls keep retained damage fixed to `damage`.
    /// The caller must ensure an ancestor clip or the content itself contains every changed output
    /// pixel inside that region; this method does not insert a clip. Content, scale, rotation, or
    /// clip changes must replace/reinsert the scene or use the normal transform path.
    pub fn insert_bounded_scene(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        canvas: Rc<Canvas>,
        transform: Affine,
        damage: Rect,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Scene {
                canvas,
                transform,
                translation_damage: Some(damage),
            },
        });
        self
    }

    pub fn insert_group(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Group,
        });
        self
    }

    pub fn insert_layer(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        layer: RetainedLayerDescriptor,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Layer(layer),
        });
        self
    }

    pub fn replace_scene(&mut self, id: RetainedNodeId, canvas: Rc<Canvas>) -> &mut Self {
        self.mutations.push(Mutation::ReplaceScene { id, canvas });
        self
    }

    pub fn set_transform(&mut self, id: RetainedNodeId, transform: Affine) -> &mut Self {
        self.mutations.push(Mutation::SetTransform {
            id,
            transform,
            translation_damage: None,
        });
        self
    }

    /// Updates a retained scene translation while keeping its output damage fixed to `damage`.
    ///
    /// The affine linear coefficients must match the currently installed transform. Use
    /// [`Self::set_transform`] for scale, rotation, or skew changes. The caller must keep every
    /// changed output pixel within `damage`, normally with an ancestor clip.
    pub fn set_bounded_translation(
        &mut self,
        id: RetainedNodeId,
        transform: Affine,
        damage: Rect,
    ) -> &mut Self {
        self.mutations.push(Mutation::SetTransform {
            id,
            transform,
            translation_damage: Some(damage),
        });
        self
    }

    pub fn update_layer(
        &mut self,
        id: RetainedNodeId,
        layer: RetainedLayerDescriptor,
    ) -> &mut Self {
        self.mutations.push(Mutation::UpdateLayer { id, layer });
        self
    }

    pub fn reparent(
        &mut self,
        id: RetainedNodeId,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
    ) -> &mut Self {
        self.mutations
            .push(Mutation::Reparent { id, parent, before });
        self
    }

    pub fn move_before(&mut self, id: RetainedNodeId, sibling: RetainedNodeId) -> &mut Self {
        self.mutations.push(Mutation::MoveBefore { id, sibling });
        self
    }

    pub fn remove_subtree(&mut self, id: RetainedNodeId) -> &mut Self {
        self.mutations.push(Mutation::Remove { id });
        self
    }

    pub fn resize(&mut self, width: u32, height: u32, scale: f32) -> &mut Self {
        self.mutations.push(Mutation::Resize {
            width,
            height,
            scale,
        });
        self
    }

    pub fn invalidate_rect(&mut self, rect: Rect) -> &mut Self {
        self.mutations.push(Mutation::InvalidateRect(rect));
        self
    }

    pub fn invalidate_all(&mut self) -> &mut Self {
        self.mutations.push(Mutation::InvalidateAll);
        self
    }

    pub fn commit(&mut self) -> Result<SceneVersion, RetainedSceneError> {
        self.scene
            .commit_mutations(std::mem::take(&mut self.mutations))
    }
}
