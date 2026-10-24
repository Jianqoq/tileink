//! Infallible color lowering for usvg-normalized matrices and channel functions.
use crate::{
    Filter,
    shared::layer::filter::{
        COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, ComponentTransferTable,
    },
};

#[allow(
    clippy::expect_used,
    reason = "usvg normalizes color matrices to exactly twenty coefficients before lowering"
)]
pub(super) fn color_matrix_to_filter(matrix: &usvg::filter::ColorMatrixKind) -> Option<Filter> {
    match matrix {
        usvg::filter::ColorMatrixKind::Saturate(amount) => Some(Filter::Saturate(amount.get())),
        usvg::filter::ColorMatrixKind::HueRotate(angle) => Some(Filter::HueRotate(*angle)),
        usvg::filter::ColorMatrixKind::Matrix(values) => {
            // usvg normalizes every matrix to twenty coefficients. Capture that
            // contract once; recognition and copying cannot encounter short rows.
            let values: &[f32; 20] = values.as_slice().try_into().expect(
                "usvg normalizes color matrices to exactly twenty coefficients before lowering",
            );
            if matrix_is_identity(values) {
                None
            } else if let Some(amount) = standard_grayscale_amount(values) {
                Some(Filter::Grayscale(amount))
            } else if let Some(amount) = standard_sepia_amount(values) {
                Some(Filter::Sepia(amount))
            } else {
                Some(Filter::ColorMatrix(*values))
            }
        }
        usvg::filter::ColorMatrixKind::LuminanceToAlpha => Some(Filter::ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2126,
            0.7152, 0.0722, 0.0, 0.0,
        ])),
    }
}

pub(super) fn component_transfer_to_filter(
    transfer: &usvg::filter::ComponentTransfer,
) -> Option<Filter> {
    let r = transfer.func_r();
    let g = transfer.func_g();
    let b = transfer.func_b();
    let a = transfer.func_a();

    if transfer_is_identity(r)
        && transfer_is_identity(g)
        && transfer_is_identity(b)
        && transfer_is_identity(a)
    {
        return None;
    }

    let opacity_amount =
        (transfer_is_identity(r) && transfer_is_identity(g) && transfer_is_identity(b))
            .then(|| transfer_opacity_amount(a))
            .flatten();
    if let Some(amount) = opacity_amount {
        return Some(Filter::Opacity(amount));
    }

    if transfer_is_identity(a) {
        if let Some((slope, intercept)) = same_linear_rgb(r, g, b) {
            if nearly_eq(intercept, 0.0) {
                return Some(Filter::Brightness(slope));
            }
            if nearly_eq(intercept, 0.5 - 0.5 * slope) {
                return Some(Filter::Contrast(slope));
            }
        }

        if let Some(amount) = same_invert_table_rgb(r, g, b) {
            return Some(Filter::Invert(amount));
        }
    }

    Some(Filter::ComponentTransfer(component_transfer_table(
        r, g, b, a,
    )))
}

fn transfer_opacity_amount(transfer: &usvg::filter::TransferFunction) -> Option<f32> {
    if let Some([start, amount]) = transfer_table_pair(transfer) {
        return nearly_eq(start, 0.0).then_some(amount);
    }
    let (slope, intercept) = transfer_linear(transfer)?;
    nearly_eq(intercept, 0.0).then_some(slope)
}

fn same_invert_table_rgb(
    r: &usvg::filter::TransferFunction,
    g: &usvg::filter::TransferFunction,
    b: &usvg::filter::TransferFunction,
) -> Option<f32> {
    let [r0, r1] = transfer_table_pair(r)?;
    let [g0, g1] = transfer_table_pair(g)?;
    let [b0, b1] = transfer_table_pair(b)?;
    (nearly_eq(r0, g0)
        && nearly_eq(r0, b0)
        && nearly_eq(r1, g1)
        && nearly_eq(r1, b1)
        && nearly_eq(r1, 1.0 - r0))
    .then_some(r0)
}

fn same_linear_rgb(
    r: &usvg::filter::TransferFunction,
    g: &usvg::filter::TransferFunction,
    b: &usvg::filter::TransferFunction,
) -> Option<(f32, f32)> {
    let (rs, ri) = transfer_linear(r)?;
    let (gs, gi) = transfer_linear(g)?;
    let (bs, bi) = transfer_linear(b)?;
    (nearly_eq(rs, gs) && nearly_eq(rs, bs) && nearly_eq(ri, gi) && nearly_eq(ri, bi))
        .then_some((rs, ri))
}

const fn transfer_linear(transfer: &usvg::filter::TransferFunction) -> Option<(f32, f32)> {
    match transfer {
        usvg::filter::TransferFunction::Linear { slope, intercept } => Some((*slope, *intercept)),
        _ => None,
    }
}

fn transfer_table_pair(transfer: &usvg::filter::TransferFunction) -> Option<[f32; 2]> {
    match transfer {
        usvg::filter::TransferFunction::Table(values) if values.len() == 2 => {
            Some([values[0], values[1]])
        }
        _ => None,
    }
}

fn transfer_is_identity(transfer: &usvg::filter::TransferFunction) -> bool {
    match transfer {
        usvg::filter::TransferFunction::Identity => true,
        // usvg uses empty vectors for missing or invalid tableValues; treat them as no-op.
        usvg::filter::TransferFunction::Table(values)
        | usvg::filter::TransferFunction::Discrete(values)
            if values.is_empty() =>
        {
            true
        }
        usvg::filter::TransferFunction::Linear { slope, intercept } => {
            nearly_eq(*slope, 1.0) && nearly_eq(*intercept, 0.0)
        }
        usvg::filter::TransferFunction::Table(values) if values.len() == 2 => {
            nearly_eq(values[0], 0.0) && nearly_eq(values[1], 1.0)
        }
        _ => false,
    }
}

fn component_transfer_table(
    r: &usvg::filter::TransferFunction,
    g: &usvg::filter::TransferFunction,
    b: &usvg::filter::TransferFunction,
    a: &usvg::filter::TransferFunction,
) -> Box<ComponentTransferTable> {
    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    fill_component_transfer_channel(&mut table, 0, r);
    fill_component_transfer_channel(&mut table, 1, g);
    fill_component_transfer_channel(&mut table, 2, b);
    fill_component_transfer_channel(&mut table, 3, a);
    table
}

fn fill_component_transfer_channel(
    table: &mut ComponentTransferTable,
    channel: usize,
    transfer: &usvg::filter::TransferFunction,
) {
    let base = channel * COMPONENT_TRANSFER_TABLE_SIZE;
    for i in 0..COMPONENT_TRANSFER_TABLE_SIZE {
        let value = transfer_function_value(transfer, i as f32 / 255.0).clamp(0.0, 1.0);
        table[base + i] = (value * 255.0 + 0.5) as u32;
    }
}

fn transfer_function_value(transfer: &usvg::filter::TransferFunction, value: f32) -> f32 {
    match transfer {
        usvg::filter::TransferFunction::Identity => value,
        usvg::filter::TransferFunction::Table(values) => table_transfer_value(values, value),
        usvg::filter::TransferFunction::Discrete(values) => discrete_transfer_value(values, value),
        usvg::filter::TransferFunction::Linear { slope, intercept } => slope * value + intercept,
        usvg::filter::TransferFunction::Gamma {
            amplitude,
            exponent,
            offset,
        } => amplitude * value.powf(*exponent) + offset,
    }
}

fn table_transfer_value(values: &[f32], value: f32) -> f32 {
    match values.len() {
        0 => value,
        1 => values[0],
        len => {
            let position = value.clamp(0.0, 1.0) * (len - 1) as f32;
            let left = position.floor() as usize;
            let right = (left + 1).min(len - 1);
            lerp(values[left], values[right], position - left as f32)
        }
    }
}

fn discrete_transfer_value(values: &[f32], value: f32) -> f32 {
    match values.len() {
        0 => value,
        len => {
            values[(value.clamp(0.0, 1.0) * len as f32)
                .floor()
                .min((len - 1) as f32) as usize]
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn matrix_is_identity(values: &[f32; 20]) -> bool {
    const IDENTITY: [f32; 20] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0,
    ];
    matrix_matches(values, &IDENTITY)
}

fn standard_grayscale_amount(values: &[f32; 20]) -> Option<f32> {
    let amount = 1.0 - (values[0] - 0.2126) / 0.7874;
    let expected = [
        0.2126 + 0.7874 * (1.0 - amount),
        0.7152 - 0.7152 * (1.0 - amount),
        0.0722 - 0.0722 * (1.0 - amount),
        0.0,
        0.0,
        0.2126 - 0.2126 * (1.0 - amount),
        0.7152 + 0.2848 * (1.0 - amount),
        0.0722 - 0.0722 * (1.0 - amount),
        0.0,
        0.0,
        0.2126 - 0.2126 * (1.0 - amount),
        0.7152 - 0.7152 * (1.0 - amount),
        0.0722 + 0.9278 * (1.0 - amount),
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ];
    matrix_matches(values, &expected).then_some(amount.clamp(0.0, 1.0))
}

fn standard_sepia_amount(values: &[f32; 20]) -> Option<f32> {
    let amount = 1.0 - (values[0] - 0.393) / 0.607;
    let expected = [
        0.393 + 0.607 * (1.0 - amount),
        0.769 - 0.769 * (1.0 - amount),
        0.189 - 0.189 * (1.0 - amount),
        0.0,
        0.0,
        0.349 - 0.349 * (1.0 - amount),
        0.686 + 0.314 * (1.0 - amount),
        0.168 - 0.168 * (1.0 - amount),
        0.0,
        0.0,
        0.272 - 0.272 * (1.0 - amount),
        0.534 - 0.534 * (1.0 - amount),
        0.131 + 0.869 * (1.0 - amount),
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ];
    matrix_matches(values, &expected).then_some(amount.clamp(0.0, 1.0))
}

fn matrix_matches(values: &[f32; 20], expected: &[f32; 20]) -> bool {
    values
        .iter()
        .zip(expected)
        .all(|(value, expected)| nearly_eq(*value, *expected))
}

pub(super) fn nearly_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0e-4
}

#[cfg(test)]
mod tests;
