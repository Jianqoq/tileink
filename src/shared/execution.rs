use crate::shared::layer::Layer;

pub(crate) enum DisplayItem {
    Draw(usize),
    BeginLayer(Layer),
    EndLayer,
}
