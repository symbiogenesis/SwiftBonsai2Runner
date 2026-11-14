//! Deterministic output-subspace ridge fitting with bounded scalar work.
#![expect(
    clippy::suboptimal_flops,
    reason = "The declared scalar fitter retains explicit multiplication/addition rounding in its reductions and triangular recurrences."
)]

use std::time::Instant;

use super::{Row, Split};

pub(super) struct FitRow<'a> {
    pub(super) input: &'a [f32],
    pub(super) target: Vec<f64>,
}

pub(super) const MAX_WORK: u64 = 300_000_000;

pub(super) fn charge(work: &mut u64, cells: usize, deadline: Instant) -> crate::Result<()> {
    *work = work
        .checked_add(u64::try_from(cells).map_err(|error| error.to_string())?)
        .ok_or("Surrogate work counter overflow.")?;
    if *work > MAX_WORK || Instant::now() >= deadline {
        return Err("Surrogate time/work budget exhausted; no artifact published.".into());
    }
    Ok(())
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

fn basis(
    rows: &[FitRow<'_>],
    rank: usize,
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<Vec<Vec<f64>>> {
    charge(work, rows.len() * rows[0].target.len() * 2, deadline)?;
    let mut residuals: Vec<Vec<_>> = rows.iter().map(|row| row.target.clone()).collect();
    let largest_energy = residuals
        .iter()
        .map(|row| dot(row, row))
        .fold(0.0_f64, f64::max);
    if largest_energy == 0.0 {
        return Err("Teacher fit targets have no nonzero output subspace.".into());
    }
    let mut result: Vec<Vec<f64>> = Vec::new();
    for _ in 0..rank {
        charge(work, residuals.len() * rows[0].target.len(), deadline)?;
        // Strict comparison keeps the earlier row in an exact tie.
        let mut pivot = 0;
        let mut energy = dot(&residuals[0], &residuals[0]);
        for (index, row) in residuals.iter().enumerate().skip(1) {
            let candidate = dot(row, row);
            if candidate > energy {
                pivot = index;
                energy = candidate;
            }
        }
        if energy <= largest_energy * 1e-20 {
            break;
        }
        let mut direction = residuals[pivot].clone();
        // Reorthogonalization controls cancellation after dependent targets.
        for previous in &result {
            let projection = dot(&direction, previous);
            for (value, &basis) in direction.iter_mut().zip(previous) {
                *value -= projection * basis;
            }
            charge(work, direction.len() * 2, deadline)?;
        }
        let norm = dot(&direction, &direction).sqrt();
        if !norm.is_finite() || norm <= largest_energy.sqrt() * 1e-10 {
            break;
        }
        for value in &mut direction {
            *value /= norm;
        }
        charge(work, direction.len() * 2, deadline)?;
        for row in &mut residuals {
            let projection = dot(row, &direction);
            for (value, &basis) in row.iter_mut().zip(&direction) {
                *value -= projection * basis;
            }
            charge(work, row.len() * 2, deadline)?;
        }
        result.push(direction);
    }
    if result.is_empty() {
        return Err("Teacher output subspace is numerically degenerate.".into());
    }
    Ok(result)
}

fn cholesky(matrix: &mut [Vec<f64>], deadline: Instant, work: &mut u64) -> crate::Result<()> {
    let mut row = 0;
    while row < matrix.len() {
        let mut column = 0;
        while column <= row {
            let mut value = matrix[row][column];
            for (&left, &right) in matrix[row][..column].iter().zip(&matrix[column][..column]) {
                value -= left * right;
            }
            charge(work, column + 1, deadline)?;
            matrix[row][column] = if row == column {
                if !value.is_finite() || value <= 0.0 {
                    return Err("Ridge system is not numerically positive definite.".into());
                }
                value.sqrt()
            } else {
                value / matrix[column][column]
            };
            column += 1;
        }
        row += 1;
    }
    Ok(())
}

fn solve(
    matrix: &[Vec<f64>],
    target: &mut [f64],
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<()> {
    let mut row = 0;
    while row < target.len() {
        let mut value = target[row];
        for (&coefficient, &previous) in matrix[row][..row].iter().zip(&target[..row]) {
            value -= coefficient * previous;
        }
        target[row] = value / matrix[row][row];
        charge(work, row + 1, deadline)?;
        row += 1;
    }
    while row > 0 {
        row -= 1;
        let mut value = target[row];
        for (coefficients, &later) in matrix[row + 1..].iter().zip(&target[row + 1..]) {
            value -= coefficients[row] * later;
        }
        target[row] = value / matrix[row][row];
        charge(work, target.len() - row, deadline)?;
    }
    Ok(())
}

pub(super) struct Solution {
    pub(super) a: Vec<Vec<f32>>,
    pub(super) b: Vec<Vec<f32>>,
    pub(super) work: u64,
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "F64 fitted factors are exported as F32 and rejected if narrowing becomes nonfinite."
)]
fn narrow(values: Vec<f64>) -> crate::Result<Vec<f32>> {
    let values: Vec<_> = values.into_iter().map(|value| value as f32).collect();
    if values.iter().any(|value| !value.is_finite()) {
        return Err("Exported factors must be finite F32.".into());
    }
    Ok(values)
}

pub(super) fn fit(
    rows: &[&Row],
    rank: usize,
    ridge: f64,
    deadline: Instant,
) -> crate::Result<Solution> {
    let mut work = 0;
    charge(&mut work, rows.len() * rows[0].target.len(), deadline)?;
    let rows: Vec<_> = rows
        .iter()
        .map(|row| FitRow {
            input: &row.input,
            target: row.target.iter().copied().map(f64::from).collect(),
        })
        .collect();
    fit_data(&rows, rank, ridge, deadline, &mut work)
}

pub(super) fn fit_data(
    rows: &[FitRow<'_>],
    rank: usize,
    ridge: f64,
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<Solution> {
    charge(work, 0, deadline)?;
    let output_basis = basis(rows, rank, deadline, work)?;
    let mut gram = vec![vec![0.0; rows.len()]; rows.len()];
    let mut row = 0;
    while row < rows.len() {
        let mut column = 0;
        while column <= row {
            let value = rows[row]
                .input
                .iter()
                .zip(rows[column].input)
                .map(|(&left, &right)| f64::from(left) * f64::from(right))
                .sum::<f64>();
            gram[row][column] = value;
            gram[column][row] = value;
            charge(work, rows[row].input.len(), deadline)?;
            column += 1;
        }
        gram[row][row] += ridge;
        charge(work, 1, deadline)?;
        row += 1;
    }
    cholesky(&mut gram, deadline, work)?;
    let mut a = Vec::with_capacity(output_basis.len());
    let mut b = Vec::with_capacity(output_basis.len());
    for direction in output_basis {
        let mut coefficients = Vec::with_capacity(rows.len());
        for row in rows {
            coefficients.push(
                row.target
                    .iter()
                    .zip(&direction)
                    .map(|(&value, &basis)| value * basis)
                    .sum(),
            );
            charge(work, direction.len(), deadline)?;
        }
        solve(&gram, &mut coefficients, deadline, work)?;
        let mut input_factor = vec![0.0; rows[0].input.len()];
        for (row, &coefficient) in rows.iter().zip(&coefficients) {
            for (value, &input) in input_factor.iter_mut().zip(row.input) {
                *value += coefficient * f64::from(input);
            }
            charge(work, input_factor.len(), deadline)?;
        }
        charge(work, (input_factor.len() + direction.len()) * 2, deadline)?;
        a.push(narrow(input_factor)?);
        b.push(narrow(direction)?);
    }
    Ok(Solution { a, b, work: *work })
}

pub(super) fn prediction(
    input: &[f32],
    a: &[Vec<f32>],
    b: &[Vec<f32>],
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<Vec<f64>> {
    let mut prediction = vec![0.0; b[0].len()];
    for (input_factor, output_factor) in a.iter().zip(b) {
        let scalar = input_factor
            .iter()
            .zip(input)
            .map(|(&factor, &input)| f64::from(factor) * f64::from(input))
            .sum::<f64>();
        for (value, &factor) in prediction.iter_mut().zip(output_factor) {
            *value += scalar * f64::from(factor);
        }
        charge(work, input.len() + prediction.len(), deadline)?;
    }
    Ok(prediction)
}

pub(super) fn diagnostics(
    rows: &[Row],
    a: &[Vec<f32>],
    b: &[Vec<f32>],
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<serde_json::Value> {
    let mut totals = [(0_u32, 0.0, 0.0), (0_u32, 0.0, 0.0)];
    for row in rows {
        let prediction = prediction(&row.input, a, b, deadline, work)?;
        let squared_error = prediction
            .iter()
            .zip(&row.target)
            .map(|(&value, &target)| (value - f64::from(target)).powi(2))
            .sum::<f64>();
        let energy = row
            .target
            .iter()
            .map(|&value| f64::from(value).powi(2))
            .sum::<f64>();
        charge(work, row.target.len() * 2, deadline)?;
        let total = &mut totals[usize::from(row.split == Split::Development)];
        total.0 += 1;
        total.1 += squared_error;
        total.2 += energy;
    }
    let mut result = serde_json::Map::new();
    for (name, (count, error, energy)) in ["fit", "development"].into_iter().zip(totals) {
        if !error.is_finite() || !energy.is_finite() {
            return Err("Nonfinite exported-factor diagnostics.".into());
        }
        let cells = f64::from(count)
            * f64::from(u32::try_from(rows[0].target.len()).map_err(|error| error.to_string())?);
        result.insert(
            name.into(),
            serde_json::json!({"rows":count,"meanSquaredError":error / cells,
            "zeroBranchMeanSquaredError":energy / cells,
            "relativeSquaredError":if energy > 0.0 {Some(error / energy)} else {None}}),
        );
    }
    Ok(result.into())
}
