#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CubeRenderTarget {
    Main,
    Scratch(usize),
}
