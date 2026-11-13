//! Checked projection storage and borrowed-or-owned Hadamard activations.

use std::{borrow::Cow, io};

use super::WeightQuantization;

pub(super) struct Layout {
    pub(super) groups_per_row: usize,
    pub(super) bytes_per_row: usize,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn rows(row: usize, count: usize, actual: usize, message: &'static str) -> io::Result<()> {
    if row.checked_mul(count) != Some(actual) {
        return Err(invalid(message));
    }
    Ok(())
}

fn ternary(
    input: usize,
    output: usize,
    group: usize,
    per_byte: usize,
    scales: &[f32],
    planes: &[&[u8]],
    message: &'static str,
) -> io::Result<Layout> {
    if group == 0 || !group.is_multiple_of(per_byte) || !input.is_multiple_of(group) {
        return Err(invalid(message));
    }
    let layout = Layout {
        groups_per_row: input / group,
        bytes_per_row: input / per_byte,
    };
    rows(layout.groups_per_row, output, scales.len(), message)?;
    for plane in planes {
        rows(layout.bytes_per_row, output, plane.len(), message)?;
    }
    Ok(layout)
}

pub(super) fn validate(
    input: usize,
    output: usize,
    weight: &WeightQuantization,
) -> io::Result<Layout> {
    if input == 0 || output == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid LinearProjection dimensions.",
        ));
    }
    match weight {
        WeightQuantization::F32(values) => {
            rows(
                input,
                output,
                values.len(),
                "F32 weight buffer size mismatch.",
            )?;
            Ok(Layout {
                groups_per_row: input,
                bytes_per_row: 0,
            })
        }
        WeightQuantization::Q8_0 { blocks } => {
            let message = "Q8_0 block count mismatch.";
            if !input.is_multiple_of(32) {
                return Err(invalid(message));
            }
            rows(input / 32, output, blocks.len(), message)?;
            Ok(Layout {
                groups_per_row: input / 32,
                bytes_per_row: 0,
            })
        }
        WeightQuantization::Packed { ggml_type, bytes } => {
            let message = "Invalid packed projection size.";
            let (block, size) = crate::weights::packed_layout(*ggml_type)?;
            if !input.is_multiple_of(block) {
                return Err(invalid(message));
            }
            let bytes_per_row = (input / block)
                .checked_mul(size)
                .ok_or_else(|| invalid(message))?;
            rows(bytes_per_row, output, bytes.len(), message)?;
            Ok(Layout {
                groups_per_row: input / block,
                bytes_per_row,
            })
        }
        WeightQuantization::PQ2_0 {
            group_size,
            scales,
            packed_trits,
            hadamard_block,
            signs,
        } => {
            let layout = ternary(
                input,
                output,
                *group_size,
                4,
                scales,
                &[packed_trits],
                "PQ2_0 buffer dimensions mismatch.",
            )?;
            fold_dimensions(input, signs.len(), *hadamard_block)?;
            Ok(layout)
        }
        WeightQuantization::PTQ1_0 {
            group_size,
            scales,
            nz_mask,
            sign_mask,
            hadamard_block,
            signs,
        } => {
            let layout = ternary(
                input,
                output,
                *group_size,
                8,
                scales,
                &[nz_mask, sign_mask],
                "PTQ1_0 buffer dimensions mismatch.",
            )?;
            fold_dimensions(input, signs.len(), *hadamard_block)?;
            Ok(layout)
        }
    }
}

pub(super) fn fold_dimensions(input: usize, signs: usize, block: usize) -> io::Result<()> {
    if input == 0 || input != signs || !block.is_power_of_two() || !input.is_multiple_of(block) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Hadamard sign-fold dimensions.",
        ));
    }
    Ok(())
}

pub(super) fn folded<'a>(
    input: &[f32],
    pre_folded: Option<&'a [f32]>,
    signs: &[i8],
    block: usize,
) -> io::Result<Cow<'a, [f32]>> {
    pre_folded
        .filter(|values| values.len() == input.len())
        .map_or_else(
            || super::apply_hadamard_sign_fold(input, signs, block).map(Cow::Owned),
            |existing| Ok(Cow::Borrowed(existing)),
        )
}

#[cfg(test)]
mod tests;
