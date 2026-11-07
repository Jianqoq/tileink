//! Sampling/mask geometry follows scene placement; kernels keep their physical pixel units.
use super::*;

/// A materialized leaf has fixed command topology until it is re-encoded with `Canvas::reset`.
/// Snapshot only effect descriptors so every pose starts from authored metadata, including
/// the first non-translation pose. Clones share immutable sources, not mutable placements.
#[derive(Clone, Default)]
pub(super) enum RetainedEffects {
    #[default]
    Uncaptured,
    Empty,
    Captured(SharedRc<[(usize, usize, Command)]>),
}
impl RetainedEffects {
    pub(super) fn place(&mut self, lists: &mut [CommandList], transform: GpuAffine) {
        if matches!(self, Self::Uncaptured) {
            let sources: Vec<_> = lists
                .iter()
                .enumerate()
                .flat_map(|(list, commands)| {
                    commands
                        .commands
                        .iter()
                        .enumerate()
                        .filter(|(_, command)| has_region(command))
                        .map(move |(index, command)| (list, index, command.clone()))
                })
                .collect();
            *self = if sources.is_empty() {
                Self::Empty
            } else {
                Self::Captured(sources.into())
            };
        }
        if let Self::Captured(sources) = self {
            for (list, index, original) in sources.iter() {
                let mut command = original.clone();
                place(&mut command, transform);
                lists[*list].commands[*index] = command;
            }
        }
    }
}
const fn has_region(command: &Command) -> bool {
    matches!(
        command,
        Command::Layer {
            layer: Layer::Filter { .. } | Layer::Backdrop { .. },
            ..
        } | Command::MaskLayer { .. }
    )
}

/// This changes sampling geometry, never blur strengths or other authored kernel values.
/// General-affine procedural filter coordinate remapping is not provided by this operation;
/// see `docs/native/scene-effect-placement.md` for the physical filter descriptor contract.
pub(super) fn place(command: &mut Command, transform: GpuAffine) {
    if !has_region(command) {
        return;
    }
    let affine = Affine::new(
        [
            transform.a,
            transform.b,
            transform.c,
            transform.d,
            transform.e,
            transform.f,
        ]
        .map(f64::from),
    );
    if [transform.a, transform.b, transform.c, transform.d] == [1., 0., 0., 1.] {
        // Preserve the existing exact translation contract for spatial filter/paint parameters.
        if transform.e != 0. || transform.f != 0. {
            *command = SceneOffset::new(Point::new(f64::from(transform.e), f64::from(transform.f)))
                .command(command.clone());
        }
        return;
    }
    match command {
        Command::Layer {
            layer: Layer::Filter { sample_region, .. } | Layer::Backdrop { sample_region, .. },
            ..
        } => transform_region(sample_region, affine),
        Command::MaskLayer { layer, .. } => transform_region(&mut layer.region, affine),
        _ => {}
    }
}
fn transform_region(region: &mut Region, affine: Affine) {
    match region {
        Region::Path { transform, .. } => *transform = affine * *transform,
        Region::Rect { rect, radius } => {
            let [a, b, c, d, _, _] = affine.as_coeffs();
            if b == 0. && c == 0. && (a.abs() == d.abs() || radius.is_zero()) {
                *rect = affine.transform_rect_bbox(*rect);
                let scale = a.abs() as f32;
                radius.top_left *= scale;
                radius.top_right *= scale;
                radius.bottom_left *= scale;
                radius.bottom_right *= scale;
                if a < 0. {
                    std::mem::swap(&mut radius.top_left, &mut radius.top_right);
                    std::mem::swap(&mut radius.bottom_left, &mut radius.bottom_right);
                }
                if d < 0. {
                    std::mem::swap(&mut radius.top_left, &mut radius.bottom_left);
                    std::mem::swap(&mut radius.top_right, &mut radius.bottom_right);
                }
            } else {
                // A rotated/sheared or elliptically scaled rounded box is a transformed path,
                // not its axis-aligned bounding box (which would change mask coverage).
                *region = Region::path(Canvas::rounded_rect_path(*rect, *radius, 0.1), affine, 0.1);
            }
        }
    }
}
