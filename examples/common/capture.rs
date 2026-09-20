//! Scoped execution of the existing examples on an explicitly selected GPU.
//!
//! The ordinary PNG runner keeps its own cache. A reference run captures raw
//! pixels, pins font bytes, and rejects incomplete or ambiguous output catalogs.
use super::fonts::Snapshot;
use super::rendering::{Backend, Renderer, SceneRenderer};

use peniko::Color;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    rc::Rc,
};
use tileink::{Image, TextFontSystem};

#[cfg(feature = "wgpu")]
use tileink::WgpuRenderer;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

struct Session {
    backend: Backend,
    inputs: Rc<Inputs>,
    renderers: HashMap<(u32, u32), Renderer>,
    frames: Frames,
    pipelines: Vec<serde_json::Value>,
    precompiled_dxil_seen: bool,
}

/// All mutable file inputs are captured before routes begin rendering.
pub struct Inputs {
    pub fonts: Snapshot,
    pub svgs: BTreeMap<PathBuf, usvg::Tree>,
}
impl Inputs {
    pub fn svg(&self, path: &Path) -> Result<usvg::Tree> {
        self.svgs
            .get(&std::path::absolute(path)?)
            .cloned()
            .ok_or_else(|| format!("uncaptured example SVG: {}", path.display()).into())
    }
}

pub struct Captured {
    pub images: BTreeMap<String, Image>,
    pub pipelines: Vec<serde_json::Value>,
    pub precompiled_dxil_seen: bool,
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        SESSION.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

#[cfg(feature = "wgpu")]
pub fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    inputs: Rc<Inputs>,
    names: &[&str],
    render: impl FnOnce() -> Result<()>,
) -> Result<Captured> {
    run_backend(
        Backend::Wgpu {
            device: device.clone(),
            queue: queue.clone(),
        },
        inputs,
        names,
        render,
    )
}

#[cfg(tileink_native_runtime)]
pub fn run_native(
    context: &tileink::NativeContext,
    inputs: Rc<Inputs>,
    names: &[&str],
    render: impl FnOnce() -> Result<()>,
) -> Result<Captured> {
    run_backend(Backend::Native(context.clone()), inputs, names, render)
}

fn run_backend(
    backend: Backend,
    inputs: Rc<Inputs>,
    names: &[&str],
    render: impl FnOnce() -> Result<()>,
) -> Result<Captured> {
    let frames = Frames::new(names)?;
    SESSION.with(|slot| -> Result<()> {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return Err("nested example capture is not allowed".into());
        }
        *slot = Some(Session {
            backend,
            inputs,
            renderers: HashMap::new(),
            frames,
            pipelines: Vec::new(),
            precompiled_dxil_seen: false,
        });
        Ok(())
    })?;
    let _reset = Reset;
    render()?;
    let session = SESSION.with(|slot| slot.borrow_mut().take().unwrap());
    Ok(Captured {
        images: session.frames.finish()?,
        pipelines: session.pipelines,
        precompiled_dxil_seen: session.precompiled_dxil_seen,
    })
}

#[cfg(feature = "wgpu")]
pub(super) fn new_renderer(width: u32, height: u32, clear: Color) -> Option<WgpuRenderer> {
    SESSION.with(|slot| {
        slot.borrow().as_ref().map(|s| match &s.backend {
            Backend::Wgpu { device, queue } => {
                WgpuRenderer::new(device, queue, width, height, clear)
            }
            #[cfg(tileink_native_runtime)]
            Backend::Native(_) => {
                panic!("a native capture cannot create an implicit wgpu renderer")
            }
        })
    })
}

pub(super) fn font_system() -> Option<TextFontSystem> {
    SESSION.with(|slot| slot.borrow().as_ref().map(|s| s.inputs.fonts.font_system()))
}

pub(super) fn svg_tree(path: &Path) -> Option<Result<usvg::Tree>> {
    SESSION.with(|slot| slot.borrow().as_ref().map(|s| s.inputs.svg(path)))
}

#[cfg(feature = "wgpu")]
pub fn record_pipelines(name: &str, renderer: &WgpuRenderer) {
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            session.precompiled_dxil_seen |= renderer.precompiled_dxil_pipeline_count() > 0;
            session.pipelines.push(serde_json::json!({
                "workload": name,
                "compiled_pipelines": renderer.pipeline_compilation_epoch(),
                "precompiled_dxil_pipelines": renderer.precompiled_dxil_pipeline_count(),
            }));
        }
    });
}

pub(super) fn render(
    name: &str,
    width: u32,
    height: u32,
    clear: Color,
    render: &mut impl FnMut(&mut dyn SceneRenderer) -> Result<()>,
) -> Option<Result<()>> {
    let renderer = SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let session = slot.as_mut()?;
        Some((|| {
            session.frames.check_name(name)?;
            match session.renderers.remove(&(width, height)) {
                Some(renderer) => Ok(renderer),
                None => session.backend.create(width, height),
            }
        })())
    })?;
    Some((|| {
        let mut renderer = renderer?;
        renderer.set_clear_color(clear);
        // Release the session borrow before executing user scene/text closures.
        render(renderer.scene_renderer())?;
        let image = renderer.image()?;
        match &renderer {
            #[cfg(feature = "wgpu")]
            Renderer::Wgpu(renderer) => record_pipelines(name, renderer),
            #[cfg(tileink_native_runtime)]
            Renderer::Native(_) => {}
        }
        SESSION.with(|slot| -> Result<()> {
            let mut slot = slot.borrow_mut();
            let session = slot.as_mut().unwrap();
            session.frames.insert(name, image)?;
            session.renderers.insert((width, height), renderer);
            Ok(())
        })
    })())
}

struct Frames {
    expected: BTreeSet<String>,
    images: BTreeMap<String, Image>,
}
impl Frames {
    fn new(names: &[&str]) -> Result<Self> {
        let expected: BTreeSet<String> = names.iter().map(|name| (*name).to_owned()).collect();
        if expected.is_empty()
            || expected.len() != names.len()
            || names.iter().any(|name| name.is_empty())
        {
            return Err("example capture needs a nonempty, unique output catalog".into());
        }
        Ok(Self {
            expected,
            images: BTreeMap::new(),
        })
    }
    fn check_name(&self, name: &str) -> Result<()> {
        if !self.expected.contains(name) {
            return Err(format!("unexpected example output: {name}").into());
        }
        if self.images.contains_key(name) {
            return Err(format!("duplicate example output: {name}").into());
        }
        Ok(())
    }
    fn insert(&mut self, name: &str, image: Image) -> Result<()> {
        self.check_name(name)?;
        self.images.insert(name.to_owned(), image);
        Ok(())
    }
    fn finish(self) -> Result<BTreeMap<String, Image>> {
        let missing: Vec<_> = self
            .expected
            .iter()
            .filter(|name| !self.images.contains_key(*name))
            .collect();
        if !missing.is_empty() {
            return Err(format!("missing example outputs: {missing:?}").into());
        }
        Ok(self.images)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image() -> Image {
        Image {
            width: 1,
            height: 1,
            pixels: vec![0x01030201],
        }
    }
    #[test]
    fn capture_requires_exact_output_catalog() -> Result<()> {
        assert!(Frames::new(&[]).is_err());
        assert!(Frames::new(&[""]).is_err());
        assert!(Frames::new(&["a", "a"]).is_err());
        let mut frames = Frames::new(&["a", "b"])?;
        assert!(frames.insert("unknown", image()).is_err());
        frames.insert("a", image())?;
        assert!(frames.insert("a", image()).is_err());
        assert!(frames.finish().is_err());
        Ok(())
    }
    #[test]
    fn capture_preserves_raw_channels_and_catalog_order() -> Result<()> {
        let mut frames = Frames::new(&["a", "b"])?;
        frames.insert(
            "b",
            Image {
                width: 1,
                height: 1,
                pixels: vec![0x00030201],
            },
        )?;
        frames.insert("a", image())?;
        let images = frames.finish()?;
        assert_eq!(
            images.keys().map(String::as_str).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(images["a"].pixels, [0x01030201]);
        assert_eq!(images["b"].pixels, [0x00030201]);
        Ok(())
    }
}
