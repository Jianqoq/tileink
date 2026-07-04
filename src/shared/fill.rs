#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillRule {
    NonZero = 0,
    EvenOdd = 1,
}
