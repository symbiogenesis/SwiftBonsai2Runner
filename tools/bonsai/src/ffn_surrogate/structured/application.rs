//! Replay exported factors using the scalar consumer's declared F32 rounding.

use super::{Request, Split, Totals, Value, WIDTH, solver};
use std::time::Instant;

#[expect(
    clippy::suboptimal_flops,
    reason = "The scalar consumer rounds every F32 product, input-order dot addition and rank-order retained-branch addition separately; FMA changes the profile."
)]
fn branch(
    input: &[f32],
    retained: &[f32],
    solution: &solver::Solution,
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<Vec<f32>> {
    let mut output = retained.to_vec();
    solver::charge(work, output.len(), deadline)?;
    for (a, b) in solution.a.iter().zip(&solution.b) {
        let mut dot = 0.0_f32;
        for (&factor, &value) in a.iter().zip(input) {
            dot += factor * value;
        }
        if !dot.is_finite() {
            return Err(
                "Nonfinite source-order F32 compensation dot; no artifact published.".into(),
            );
        }
        for (value, &factor) in output.iter_mut().zip(b) {
            *value += dot * factor;
        }
        solver::charge(work, input.len() + output.len() * 2, deadline)?;
        if output.iter().any(|value| !value.is_finite()) {
            return Err(
                "Nonfinite source-order F32 compensated branch; no artifact published.".into(),
            );
        }
    }
    Ok(output)
}

pub(super) fn diagnostics(
    request: &Request,
    solution: &solver::Solution,
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<([Totals; 2], Value)> {
    let mut totals = [Totals::default(); 2];
    for row in &request.rows {
        let output = branch(&row.input, &row.retained_branch, solution, deadline, work)?;
        let total = &mut totals[usize::from(row.split == Split::Development)];
        total.rows += 1;
        for (((&teacher, &retained), &original), &output) in row
            .teacher_target
            .iter()
            .zip(&row.retained_branch)
            .zip(&row.original_prefix_teacher_target)
            .zip(&output)
        {
            let teacher = f64::from(teacher);
            let retained = f64::from(retained);
            let original = f64::from(original);
            let output = f64::from(output);
            total.teacher_energy += teacher.powi(2);
            total.original_teacher_energy += original.powi(2);
            total.uncompensated_error += (teacher - retained).powi(2);
            total.compensated_error += (teacher - output).powi(2);
            total.prefix_drift += (teacher - original).powi(2);
            total.original_prefix_error += (output - original).powi(2);
        }
        solver::charge(work, WIDTH * 6, deadline)?;
    }
    let result = super::summarize(&totals)?;
    Ok((totals, result))
}

pub(super) fn combined_screen(algebraic: &Value, application: &Value) -> crate::Result<Value> {
    let mut result = algebraic.clone();
    let reasons = result["rejectionReasons"]
        .as_array_mut()
        .ok_or("Missing algebraic rejection reasons.")?;
    for reason in application["rejectionReasons"]
        .as_array()
        .ok_or("Missing source-order F32 rejection reasons.")?
    {
        let reason = reason
            .as_str()
            .ok_or("Malformed source-order F32 reason.")?;
        reasons.push(format!("source-order-f32-{reason}").into());
    }
    result["passed"] = (algebraic["passed"] == true && application["passed"] == true).into();
    result["requireExportedFactorF64Screen"] = true.into();
    result["requireSourceOrderF32Screen"] = true.into();
    result["factorApplicationArithmetic"] = super::APPLICATION_ARITHMETIC.into();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn solution(a: Vec<Vec<f32>>, b: Vec<Vec<f32>>) -> solver::Solution {
        solver::Solution { a, b, work: 0 }
    }

    #[test]
    fn application_rejects_f64_pass_when_f32_dot_cancels() -> crate::Result<()> {
        let solution = solution(
            vec![vec![16_777_216.0, 1.0, -16_777_216.0]],
            vec![vec![1.0]],
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut work = 0;
        let f64_output =
            solver::prediction(&[1.0; 3], &solution.a, &solution.b, deadline, &mut work)?;
        let f32_output = branch(&[1.0; 3], &[0.0], &solution, deadline, &mut work)?;
        assert_eq!(f64_output, [1.0]);
        assert_eq!(f32_output, [0.0]);
        let fit = Totals {
            teacher_energy: 1.0,
            original_teacher_energy: 1.0,
            ..Totals::default()
        };
        let algebraic = super::super::screen(&[fit; 2]);
        let application = super::super::screen(&[
            fit,
            Totals {
                uncompensated_error: 1.0,
                compensated_error: 1.0,
                ..fit
            },
        ]);
        let result = combined_screen(&algebraic, &application)?;
        assert_eq!(algebraic["passed"], true);
        assert_eq!(result["passed"], false);
        assert_eq!(result["requireSourceOrderF32Screen"], true);
        assert!(
            result["rejectionReasons"]
                .as_array()
                .ok_or("Missing reasons.")?
                .iter()
                .any(|reason| reason
                    == "source-order-f32-development-relative-squared-error-exceeds-limit")
        );
        Ok(())
    }

    #[test]
    fn application_rounds_each_rank_into_retained_branch() -> crate::Result<()> {
        let solution = solution(
            vec![vec![1.0], vec![1.0]],
            vec![vec![1.0], vec![-16_777_216.0]],
        );
        let output = branch(
            &[1.0],
            &[16_777_216.0],
            &solution,
            Instant::now() + Duration::from_secs(1),
            &mut 0,
        )?;
        assert_eq!(output, [0.0]);
        Ok(())
    }

    #[test]
    fn finite_factors_cannot_publish_nonfinite_f32_application() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let dot_overflow = solution(vec![vec![f32::MAX]], vec![vec![1.0]]);
        assert!(branch(&[2.0], &[0.0], &dot_overflow, deadline, &mut 0).is_err());
        let branch_overflow = solution(vec![vec![1.0]], vec![vec![f32::MAX]]);
        assert!(branch(&[2.0], &[0.0], &branch_overflow, deadline, &mut 0).is_err());
        let transient_overflow = solution(
            vec![vec![1.0], vec![-1.0]],
            vec![vec![f32::MAX], vec![f32::MAX]],
        );
        assert!(branch(&[1.0], &[f32::MAX], &transient_overflow, deadline, &mut 0).is_err());
    }

    #[test]
    fn application_obeys_original_work_and_time_limits() {
        let solution = solution(vec![vec![1.0]], vec![vec![1.0]]);
        assert!(branch(&[1.0], &[0.0], &solution, Instant::now(), &mut 0).is_err());
        assert!(
            branch(
                &[1.0],
                &[0.0],
                &solution,
                Instant::now() + Duration::from_secs(1),
                &mut solver::MAX_WORK
            )
            .is_err()
        );
    }
}
