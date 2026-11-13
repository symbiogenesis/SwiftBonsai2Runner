//! Erf-based GELU evaluated through convergent incomplete-gamma series/fraction.
//! This independent f64 scalar policy is explicitly distinct from GGML kernels.

#[expect(
    clippy::many_single_char_names,
    reason = "Retain the standard a/b/c/d notation of the convergent incomplete-gamma continued fraction."
)]
fn erfc_positive(value: f64) -> f64 {
    if value == 0.0 {
        return 1.0;
    }
    if value > 27.0 {
        return 0.0;
    }
    let x = value * value;
    let factor = (-x + 0.5 * x.ln() - 0.5 * std::f64::consts::PI.ln()).exp();
    if x < 1.5 {
        let mut step = 2.0;
        let mut sum = step;
        let mut power = 0.5;
        for _ in 0..128 {
            power += 1.0;
            step *= x / power;
            sum += step;
            if step.abs() <= sum.abs() * f64::EPSILON {
                break;
            }
        }
        return (1.0 - sum * factor).clamp(0.0, 1.0);
    }
    let mut b = x + 0.5;
    let mut c = 1.0e300;
    let mut d = 1.0 / b;
    let mut result = d;
    for index in 1..=256 {
        let i = f64::from(index);
        let a = -i * (i - 0.5);
        b += 2.0;
        d = a * d + b;
        if d.abs() < 1.0e-300 {
            d = 1.0e-300;
        }
        c = b + a / c;
        if c.abs() < 1.0e-300 {
            c = 1.0e-300;
        }
        d = 1.0 / d;
        let correction = d * c;
        result *= correction;
        if (correction - 1.0).abs() <= 4.0 * f64::EPSILON {
            break;
        }
    }
    (factor * result).clamp(0.0, 1.0)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The independent GELU policy explicitly rounds its f64 probability result to f32."
)]
pub(super) fn gelu(value: f32) -> f32 {
    let x = f64::from(value);
    let complement = erfc_positive(x.abs() / std::f64::consts::SQRT_2);
    (0.5 * x
        * if x < 0.0 {
            complement
        } else {
            2.0 - complement
        }) as f32
}
