use crate::scene::Scene;

pub(crate) trait Render {
    type ScanArgs<'a>;
    type CumsumArgs<'a>;
    type CoarseArgs<'a>;
    type FineArgs<'a>;
    type ExecuteArgs<'a>;
    fn render(&mut self, scene: &Scene);
    fn execute(&mut self, scene: &Scene, args: Self::ExecuteArgs<'_>);
    fn scan(&mut self, scene: &Scene, args: Self::ScanArgs<'_>);
    fn cumsum(&mut self, scene: &Scene, args: Self::CumsumArgs<'_>);
    fn coarse(&mut self, scene: &Scene, args: Self::CoarseArgs<'_>);
    fn fine(&mut self, scene: &Scene, args: Self::FineArgs<'_>);
}
