use super::super::program::scene::SceneCache;

/// Filter-local scans need persistent buffers just like the root scene. Each scan
/// gets a distinct slot, including nested and sibling filters in the same batch.
/// Reusing a single cache would overwrite resources still needed by earlier passes.
#[derive(Default)]
pub(super) struct FilterSceneCache {
    scenes: Vec<SceneCache>,
}

impl FilterSceneCache {
    pub(super) fn frame(&mut self) -> FilterScenes<'_> {
        FilterScenes {
            scenes: &mut self.scenes,
            used: 0,
        }
    }
}

pub(super) struct FilterScenes<'a> {
    scenes: &'a mut Vec<SceneCache>,
    used: usize,
}

impl FilterScenes<'_> {
    pub(super) fn next(&mut self) -> &mut SceneCache {
        if self.used == self.scenes.len() {
            self.scenes.push(SceneCache::default());
        }
        let index = self.used;
        self.used += 1;
        &mut self.scenes[index]
    }
}

impl Drop for FilterScenes<'_> {
    fn drop(&mut self) {
        // Do not retain GPU storage for filter scenes no longer present. Submitted
        // batches keep their own resource owners until completion.
        self.scenes.truncate(self.used);
    }
}
