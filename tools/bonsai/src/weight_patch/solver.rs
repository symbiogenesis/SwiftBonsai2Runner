//! Bounded primal-dual fitting with an independently evaluated numerical gap.

use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub(super) lambda: f64,
    pub(super) target_penalty: f64,
    pub(super) preserve_penalty: f64,
    pub(super) delta: f64,
    pub(super) epsilon: f64,
    pub(super) max_iterations: u32,
    pub(super) relative_gap_tolerance: f64,
    pub(super) budget: Duration,
}

pub(super) struct Row<'a> {
    pub(super) target: bool,
    pub(super) activation: &'a [f32],
}

pub(super) struct Certificate {
    pub(super) primal: f64,
    pub(super) dual: f64,
    pub(super) relative_gap: f64,
    pub(super) dual_gradient_max: f64,
    pub(super) dual_weights: Vec<f64>,
}

pub(super) struct Solution {
    pub(super) factors: Vec<f32>,
    pub(super) certificate: Certificate,
    pub(super) iterations: u32,
    pub(super) status: &'static str,
}

fn dot(row: &[f32], values: &[f64]) -> f64 {
    row.iter()
        .zip(values)
        .map(|(&left, &right)| f64::from(left) * right)
        .sum()
}

fn transpose(rows: &[Row<'_>], dual: &[f64], output: &mut [f64]) {
    output.fill(0.0);
    for (row, &weight) in rows.iter().zip(dual) {
        for (value, &activation) in output.iter_mut().zip(row.activation) {
            *value += weight * f64::from(activation);
        }
    }
}

fn shrink(value: f64, threshold: f64) -> f64 {
    value.signum() * (value.abs() - threshold).max(0.0)
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Export is explicitly F32; each rounded value is checked and recertified."
)]
fn exported(values: &[f64]) -> Result<Vec<f32>, String> {
    values
        .iter()
        .map(|&value| {
            let rounded = value as f32;
            if !rounded.is_finite() {
                return Err("The fit produced a nonfinite F32 coefficient.".into());
            }
            Ok(rounded)
        })
        .collect()
}

fn certificate(
    rows: &[Row<'_>],
    factors: &[f32],
    dual: &[f64],
    controls: Controls,
    gradient: &mut [f64],
) -> Result<Certificate, String> {
    let values: Vec<_> = factors.iter().copied().map(f64::from).collect();
    let width = u32::try_from(values.len()).map_err(|error| error.to_string())?;
    let steps = 2.0 * f64::from(width) + 4.0;
    let dot_gamma = steps * f64::EPSILON / (1.0 - steps * f64::EPSILON);
    let mut primal =
        controls.lambda * values.iter().map(|value| value.abs()).sum::<f64>() * (1.0 + dot_gamma);
    for row in rows {
        let push = dot(row.activation, &values);
        let absolute = row
            .activation
            .iter()
            .zip(&values)
            .map(|(&activation, value)| (f64::from(activation) * value).abs())
            .sum::<f64>();
        let dot_error = dot_gamma * absolute;
        primal += if row.target {
            controls.target_penalty * (push + controls.delta + dot_error).max(0.0)
        } else {
            controls.preserve_penalty * (push.abs() + dot_error - controls.epsilon).max(0.0)
        };
    }
    let terms = u32::try_from(rows.len()).map_err(|error| error.to_string())?;
    let arithmetic_steps = 2.0 * f64::from(terms) + 4.0;
    let gamma = arithmetic_steps * f64::EPSILON / (1.0 - arithmetic_steps * f64::EPSILON);
    primal *= 1.0 + gamma;
    let gradient_bound = |weights: &[f64], gradient: &mut [f64]| {
        transpose(rows, weights, gradient);
        let mut absolute_sums = vec![0.0; gradient.len()];
        for (row, &weight) in rows.iter().zip(weights) {
            for (sum, &activation) in absolute_sums.iter_mut().zip(row.activation) {
                *sum += (f64::from(activation) * weight).abs();
            }
        }
        gradient
            .iter()
            .zip(absolute_sums)
            .map(|(value, absolute)| value.abs() + gamma * absolute)
            .fold(0.0, f64::max)
    };
    let maximum = gradient_bound(dual, gradient);
    // A small inward margin keeps the evaluated dual inside the L-infinity ball.
    let scale =
        (controls.lambda / maximum.max(controls.lambda)).min(1.0) * (1.0 - 64.0 * f64::EPSILON);
    let mut dual_weights: Vec<_> = dual.iter().map(|value| value * scale).collect();
    let mut dual_gradient_max = gradient_bound(&dual_weights, gradient);
    if dual_gradient_max > controls.lambda {
        return Err("The rounded dual is outside its conservative feasibility bound.".into());
    }
    let contributions: Vec<_> = rows
        .iter()
        .zip(&dual_weights)
        .map(|(row, &value)| {
            if row.target {
                controls.delta * value
            } else {
                -controls.epsilon * value.abs()
            }
        })
        .collect();
    let absolute = contributions.iter().map(|value| value.abs()).sum::<f64>();
    let lower = contributions.iter().sum::<f64>() - gamma * absolute;
    let lower = if lower > 0.0 {
        lower
    } else {
        dual_weights.fill(0.0);
        dual_gradient_max = 0.0;
        0.0
    };
    let relative_gap = (primal - lower).max(0.0) / (1.0 + primal.abs() + lower.abs());
    if !primal.is_finite() || !lower.is_finite() || !relative_gap.is_finite() {
        return Err("The LP numerical certificate became nonfinite.".into());
    }
    if lower > primal + 1e-10 * (1.0 + primal.abs()) {
        return Err("The evaluated dual bound exceeds the primal objective.".into());
    }
    Ok(Certificate {
        primal,
        dual: lower,
        relative_gap,
        dual_gradient_max,
        dual_weights,
    })
}

/// PDHG solves the L1 plus target/preservation hinge objective. No held-out rows
/// enter this routine. The returned certificate evaluates rounded F32 factors.
#[expect(
    clippy::too_many_lines,
    reason = "The fixed PDHG recurrence and both independent stopping bounds form one operation."
)]
pub(super) fn solve(rows: &[Row<'_>], controls: Controls) -> Result<Solution, String> {
    const MAX_CELL_VISITS: u64 = 500_000_000;
    let started = Instant::now();
    let width = rows.first().ok_or("The fit has no rows.")?.activation.len();
    if width == 0
        || rows.iter().any(|row| {
            row.activation.len() != width || row.activation.iter().any(|value| !value.is_finite())
        })
    {
        return Err("LP rows must have matching finite nonempty activations.".into());
    }
    let cells = u64::try_from(width)
        .ok()
        .and_then(|width| u64::try_from(rows.len()).ok()?.checked_mul(width))
        .ok_or("LP shape overflows its work bound.")?;
    let norm = rows
        .iter()
        .flat_map(|row| row.activation)
        .map(|&value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    let mut values = vec![0.0; width];
    let mut extrapolated = values.clone();
    let mut dual = vec![0.0; rows.len()];
    let mut gradient = values.clone();
    let mut iterations = 0;
    let mut status = "iteration-limit";
    // A zero operator has a known optimum at the zero patch, but cannot supply
    // the requested activation-dependent correction.
    if norm == 0.0 {
        let factors = vec![0.0; width];
        dual.iter_mut().zip(rows).for_each(|(value, row)| {
            if row.target {
                *value = controls.target_penalty;
            }
        });
        return Ok(Solution {
            certificate: certificate(rows, &factors, &dual, controls, &mut gradient)?,
            factors,
            iterations,
            status: "no-op",
        });
    }
    if !norm.is_finite() {
        return Err("LP operator norm is not finite.".into());
    }
    let step = 0.99 / norm;
    for iteration in 1..=controls.max_iterations {
        if started.elapsed() >= controls.budget {
            status = "time-limit";
            break;
        }
        // Each iteration has K and K-transpose; gap evaluation has two more.
        if cells.saturating_mul(u64::from(iteration)).saturating_mul(4) > MAX_CELL_VISITS {
            status = "work-limit";
            break;
        }
        for (row, value) in rows.iter().zip(&mut dual) {
            let next = *value + step * dot(row.activation, &extrapolated);
            *value = if row.target {
                (next + step * controls.delta).clamp(0.0, controls.target_penalty)
            } else {
                shrink(next, step * controls.epsilon)
                    .clamp(-controls.preserve_penalty, controls.preserve_penalty)
            };
        }
        transpose(rows, &dual, &mut gradient);
        for ((value, extrapolated), &gradient) in
            values.iter_mut().zip(&mut extrapolated).zip(&gradient)
        {
            let previous = *value;
            *value = shrink(previous - step * gradient, step * controls.lambda);
            *extrapolated = 2.0 * *value - previous;
        }
        iterations = iteration;
        if iteration % 25 == 0 || iteration == controls.max_iterations {
            let factors = exported(&values)?;
            let evaluated = certificate(rows, &factors, &dual, controls, &mut gradient)?;
            if evaluated.relative_gap <= controls.relative_gap_tolerance {
                status = "converged";
                break;
            }
        }
    }
    let factors = exported(&values)?;
    let certificate = certificate(rows, &factors, &dual, controls, &mut gradient)?;
    if status == "converged" && factors.iter().all(|value| *value == 0.0) {
        status = "no-op";
    }
    Ok(Solution {
        factors,
        certificate,
        iterations,
        status,
    })
}

#[cfg(test)]
mod tests;
