use crate::{
    Canvas,
    native::runtime::{
        Result,
        compute::ComputeBatch,
        program::scene::SceneCache,
        renderer::{Execution, Images},
    },
};

#[test]
fn frame_rejects_foreign_image_upload_before_recording() -> Result<()> {
    let mut owner = ComputeBatch::new();
    let upload = Default::default();
    let images = Images::record(&mut owner, &upload)?;
    let mut other = ComputeBatch::new();
    let canvas = Canvas::new(2, 2, 1.0);
    let error = Execution::record(
        &mut SceneCache::default(),
        &mut other,
        &canvas,
        &images,
        false,
        65535,
    )
    .unwrap_err();
    assert!(error.to_string().contains("another compute batch"));
    assert!(other.resources().is_empty());
    assert!(other.commands().is_empty());
    Ok(())
}
