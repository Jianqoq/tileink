use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc as SharedArc,
};

use peniko::kurbo::Point;

use super::{Canvas, SceneAppendMode, SceneOffset};
use crate::shared::execution::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SceneCacheKey {
    element_id: u64,
    slot: u32,
}

impl SceneCacheKey {
    pub const DEFAULT_SLOT: u32 = 0;

    pub const fn new(element_id: u64, slot: u32) -> Self {
        Self { element_id, slot }
    }

    pub const fn for_element(element_id: u64) -> Self {
        Self::new(element_id, Self::DEFAULT_SLOT)
    }

    pub const fn element_id(self) -> u64 {
        self.element_id
    }

    pub const fn slot(self) -> u32 {
        self.slot
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct RetainedSceneCacheId {
    key: SceneCacheKey,
    revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct RetainedSceneInstanceId {
    scene: RetainedSceneCacheId,
    dx_bits: u64,
    dy_bits: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RetainedRootCacheId {
    logical_width: u32,
    logical_height: u32,
    scale_bits: u32,
    pub(crate) scenes: Vec<RetainedSceneInstanceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RetainedGraphCacheId {
    logical_width: u32,
    logical_height: u32,
    scale_bits: u32,
    fingerprint: u64,
}

#[derive(Default)]
pub(crate) struct RetainedSceneCache {
    pub(crate) scenes: HashMap<RetainedSceneCacheId, SharedArc<Canvas>>,
}

impl RetainedSceneCache {
    fn get_or_prepare(
        &mut self,
        key: SceneCacheKey,
        revision: u64,
        canvas: &SharedArc<Canvas>,
    ) -> SharedArc<Canvas> {
        let id = RetainedSceneCacheId { key, revision };
        self.scenes
            .retain(|cached_id, _| cached_id.key != key || cached_id.revision == revision);
        self.scenes
            .entry(id)
            .or_insert_with(|| canvas.clone())
            .clone()
    }
}

fn hash_pod_slice<T: bytemuck::Pod>(hasher: &mut DefaultHasher, values: &[T]) {
    bytemuck::cast_slice::<T, u8>(values).hash(hasher);
}

fn hash_retained_scene_id(
    key: SceneCacheKey,
    revision: u64,
    offset: (f64, f64),
    hasher: &mut DefaultHasher,
) {
    key.hash(hasher);
    revision.hash(hasher);
    offset.0.to_bits().hash(hasher);
    offset.1.to_bits().hash(hasher);
}

fn hash_command_for_retained_graph(command: &Command, hasher: &mut DefaultHasher) {
    std::mem::discriminant(command).hash(hasher);
    match command {
        Command::Draw(draw) => draw.hash(hasher),
        Command::RetainedScene {
            key,
            revision,
            offset,
            ..
        } => hash_retained_scene_id(*key, *revision, *offset, hasher),
        Command::Layer {
            draw,
            layer,
            children,
        } => {
            draw.hash(hasher);
            children.hash(hasher);
            format!("{layer:?}").hash(hasher);
        }
        Command::MaskLayer {
            layer,
            content,
            mask,
        } => {
            content.hash(hasher);
            mask.hash(hasher);
            format!("{layer:?}").hash(hasher);
        }
    }
}

impl Canvas {
    /// Appends a retained child canvas without forcing callers to flatten it immediately.
    ///
    /// The wgpu renderer materializes this into the normal draw buffers through a CPU-side
    /// retained scene cache keyed by `(element id, slot, revision)`. This is not a texture cache:
    /// retained scenes still render through the regular vector path and preserve exact quality.
    pub fn append_retained_scene(
        &mut self,
        key: SceneCacheKey,
        revision: u64,
        scene: SharedArc<Canvas>,
        pos: impl Into<Point>,
    ) {
        self.ensure_command_root();
        assert!(
            scene.command_stack.len() == 1 && scene.layer_stack.is_empty(),
            "cannot append a canvas with unclosed layers"
        );
        assert!(
            (self.scale_factor - scene.scale_factor).abs() <= f32::EPSILON,
            "cannot append canvases with different scale factors"
        );
        let offset = self.physical_point(pos.into());
        self.current_command_list_mut()
            .commands
            .push(Command::RetainedScene {
                key,
                revision,
                canvas: scene,
                offset: (offset.x, offset.y),
            });
    }

    pub(crate) fn has_retained_scenes(&self) -> bool {
        self.command_lists.iter().any(|list| {
            list.commands
                .iter()
                .any(|command| matches!(command, Command::RetainedScene { .. }))
        })
    }

    pub(crate) fn materialize_retained_scenes(&self, cache: &mut RetainedSceneCache) -> Canvas {
        if !self.has_retained_scenes() {
            return self.clone();
        }

        let mut materialized = self.clone();
        let original_list_count = materialized.command_lists.len();
        for list_ix in 0..original_list_count {
            let commands = std::mem::take(&mut materialized.command_lists[list_ix].commands);
            for command in commands {
                match command {
                    Command::RetainedScene {
                        key,
                        revision,
                        canvas,
                        offset,
                    } => {
                        let retained = cache.get_or_prepare(key, revision, &canvas);
                        materialized.append_scene_ref_to_list_unchecked(
                            &retained,
                            list_ix,
                            SceneAppendMode::MergeCurrent,
                            SceneOffset {
                                dx: offset.0,
                                dy: offset.1,
                            },
                        );
                    }
                    command => materialized.command_lists[list_ix].commands.push(command),
                }
            }
        }
        materialized
    }

    pub(crate) fn single_retained_scene(
        &self,
        cache: &mut RetainedSceneCache,
    ) -> Option<(RetainedSceneInstanceId, SharedArc<Canvas>)> {
        if !self.lines.is_empty()
            || !self.path_records.is_empty()
            || !self.draw_records.is_empty()
            || !self.brush_blob.is_empty()
            || !self.sdf_blob.is_empty()
            || !self.sdf_shadow_blob.is_empty()
            || !self.text_glyphs.is_empty()
            || !self.text_runs.is_empty()
            || self.command_lists.len() != 1
        {
            return None;
        }
        let [
            Command::RetainedScene {
                key,
                revision,
                canvas,
                offset,
            },
        ] = self.command_lists[self.root_commands].commands.as_slice()
        else {
            return None;
        };
        let offset = SceneOffset {
            dx: offset.0,
            dy: offset.1,
        };
        let id = RetainedSceneInstanceId {
            scene: RetainedSceneCacheId {
                key: *key,
                revision: *revision,
            },
            dx_bits: offset.dx.to_bits(),
            dy_bits: offset.dy.to_bits(),
        };
        let scene = cache.get_or_prepare(*key, *revision, canvas);
        let mut translated = Canvas::new(
            scene.logical_width,
            scene.logical_height,
            scene.scale_factor,
        );
        translated.append_scene_ref_unchecked(&scene, SceneAppendMode::MergeCurrent, offset);
        Some((id, SharedArc::new(translated)))
    }

    pub(crate) fn retained_root_cache_id(&self) -> Option<RetainedRootCacheId> {
        if !self.lines.is_empty()
            || !self.path_records.is_empty()
            || !self.draw_records.is_empty()
            || !self.brush_blob.is_empty()
            || !self.sdf_blob.is_empty()
            || !self.sdf_shadow_blob.is_empty()
            || !self.text_glyphs.is_empty()
            || !self.text_runs.is_empty()
            || self.command_lists.len() != 1
        {
            return None;
        }

        let mut scenes = Vec::with_capacity(self.command_lists[self.root_commands].commands.len());
        for command in &self.command_lists[self.root_commands].commands {
            let Command::RetainedScene {
                key,
                revision,
                offset,
                ..
            } = command
            else {
                return None;
            };
            scenes.push(RetainedSceneInstanceId {
                scene: RetainedSceneCacheId {
                    key: *key,
                    revision: *revision,
                },
                dx_bits: offset.0.to_bits(),
                dy_bits: offset.1.to_bits(),
            });
        }
        if scenes.is_empty() {
            return None;
        }
        Some(RetainedRootCacheId {
            logical_width: self.logical_width,
            logical_height: self.logical_height,
            scale_bits: self.scale_factor.to_bits(),
            scenes,
        })
    }

    pub(crate) fn retained_graph_cache_id(&self) -> Option<RetainedGraphCacheId> {
        if !self.has_retained_scenes() {
            return None;
        }

        let mut hasher = DefaultHasher::new();
        self.logical_width.hash(&mut hasher);
        self.logical_height.hash(&mut hasher);
        self.scale_factor.to_bits().hash(&mut hasher);
        hash_pod_slice(&mut hasher, &self.lines);
        hash_pod_slice(&mut hasher, &self.path_records);
        hash_pod_slice(&mut hasher, &self.draw_records);
        self.brush_blob.hash(&mut hasher);
        self.sdf_blob.hash(&mut hasher);
        self.sdf_shadow_blob.hash(&mut hasher);
        for glyph in &self.text_glyphs {
            format!("{:?}", glyph.cache_key).hash(&mut hasher);
            glyph.x.hash(&mut hasher);
            glyph.y.hash(&mut hasher);
        }
        for run in &self.text_runs {
            run.glyph_start.hash(&mut hasher);
            run.glyph_count.hash(&mut hasher);
        }
        self.root_commands.hash(&mut hasher);
        for list in &self.command_lists {
            list.commands.len().hash(&mut hasher);
            for command in &list.commands {
                hash_command_for_retained_graph(command, &mut hasher);
            }
        }

        Some(RetainedGraphCacheId {
            logical_width: self.logical_width,
            logical_height: self.logical_height,
            scale_bits: self.scale_factor.to_bits(),
            fingerprint: hasher.finish(),
        })
    }
}
