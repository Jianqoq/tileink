use peniko::{BlendMode, Compose, Mix};

#[derive(Clone, Debug)]
pub struct Blend {
    pub(crate) mode: BlendMode,
}

impl Blend {
    pub(crate) fn new(mix: Mix, compose: Compose) -> Self {
        Self {
            mode: BlendMode::new(mix, compose),
        }
    }
}
