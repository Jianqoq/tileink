use crate::shared::layer::region::Region;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskKind {
    Alpha,
    Luminance,
}

#[derive(Clone, Debug)]
pub struct Mask {
    pub region: Region,
    pub kind: MaskKind,
}
