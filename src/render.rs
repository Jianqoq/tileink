use crate::canvas::Canvas;

pub(crate) trait Render {
    type ScanArgs<'a>;
    type CumsumArgs<'a>;
    type CoarseArgs<'a>;
    type ExecuteArgs<'a>;
    fn render(&mut self, canvas: &Canvas);
    fn execute(&mut self, canvas: &Canvas, args: Self::ExecuteArgs<'_>);
    fn scan(&mut self, canvas: &Canvas, args: Self::ScanArgs<'_>);
    fn cumsum(&mut self, canvas: &Canvas, args: Self::CumsumArgs<'_>);
    fn coarse(&mut self, canvas: &Canvas, args: Self::CoarseArgs<'_>);
}
