//! Plan preparation and recording ownership.
use super::*;

impl PreparedScene<'_> {
    pub(crate) fn plan_handle(&self) -> Rc<ExecPlan> {
        std::rc::Rc::clone(&self.plan.plan)
    }
    pub(crate) fn size(&self) -> (u32, u32) {
        self.canvas.physical_size()
    }
    pub(crate) fn record(
        self,
        batch: &mut ComputeBatch,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        options: SceneOptions<'_>,
    ) -> Result<Scene> {
        self.cache
            .record_prepared(batch, self.canvas, text, images, options, self.plan)
    }
}

impl SceneCache {
    pub(crate) fn release_retained_plan(&mut self) {
        self.plan = None;
    }

    pub(crate) fn install_retained_plan(&mut self, canvas: &Canvas) {
        self.plan.clone_from(&canvas.compiled_plan);
    }

    pub(crate) fn prepare<'a>(&'a mut self, canvas: &'a Canvas) -> PreparedScene<'a> {
        // Preparation updates the fingerprint even when it compiles a new plan.
        // Move the old plan out first: cancellation or recording failure must not
        // pair that old plan with the new fingerprint on the next attempt.
        let mut cached = self.plan.take();
        let plan = self.preparation.prepare_plan(canvas, &mut cached);
        PreparedScene {
            cache: self,
            canvas,
            plan,
        }
    }
    pub(crate) fn record(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        limit: u32,
    ) -> Result<Scene> {
        self.prepare(canvas).record(
            batch,
            text,
            images,
            SceneOptions {
                limit,
                active: None,
            },
        )
    }

    // Localized plans carry remapped physical indices. Compiling the Canvas again
    // would sever that association and select unrelated draws or filter resources.
    /// # Safety
    /// The plan must be compiled for this Canvas or returned with it by the
    /// shared localizer; its physical draw/path/layer indices must remain paired.
    pub(crate) unsafe fn record_with_plan(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        options: SceneOptions<'_>,
        plan: Rc<ExecPlan>,
    ) -> Result<Scene> {
        // Root cause: explicit local metadata has no Canvas fingerprint. Retaining
        // the previous fingerprint would reuse this plan for an unrelated root.
        self.preparation = ScenePreparation::default();
        self.plan = None;
        let prepared = crate::render::prepare::PreparedPlan::new(canvas, plan);
        self.record_prepared(batch, canvas, text, images, options, prepared)
    }
}
