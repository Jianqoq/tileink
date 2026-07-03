use crate::canvas::Canvas;

pub(crate) trait Render {
    type ScanArgs<'a>;
    type CumsumArgs<'a>;
    type CoarseArgs<'a>;
    type ExecuteArgs<'a>;
    fn render(&mut self, scene: &Canvas);
    fn execute(&mut self, scene: &Canvas, args: Self::ExecuteArgs<'_>);
    fn scan(&mut self, scene: &Canvas, args: Self::ScanArgs<'_>);
    fn cumsum(&mut self, scene: &Canvas, args: Self::CumsumArgs<'_>);
    fn coarse(&mut self, scene: &Canvas, args: Self::CoarseArgs<'_>);
}
