use crate::canvas::Canvas;

pub(crate) trait Render {
    fn render(&mut self, canvas: &Canvas);
}
