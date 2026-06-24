#[derive(Debug, Clone, Copy, Default)]
pub struct Coverage {
    pub(crate) alphas: [(u8, u8); 32],
    pub(crate) alpha_cnt: u8,
    pub(crate) is_left: bool,
}
