//! Analytic optima exercise both the primal objective and dual feasibility.

use super::*;

fn controls() -> Controls {
    Controls {
        lambda: 1.0,
        target_penalty: 2.0,
        preserve_penalty: 2.0,
        delta: 1.0,
        epsilon: 0.0,
        max_iterations: 5_000,
        relative_gap_tolerance: 1e-6,
        budget: Duration::from_secs(2),
    }
}

#[test]
fn separated_rows_recover_known_l1_optimum() -> Result<(), String> {
    let rows = [
        Row {
            target: true,
            activation: &[1.0, 0.0],
        },
        Row {
            target: false,
            activation: &[0.0, 1.0],
        },
    ];
    let solution = solve(&rows, controls())?;
    assert_eq!(solution.status, "converged");
    assert!((f64::from(solution.factors[0]) + 1.0).abs() < 1e-5);
    assert!(solution.factors[1].abs() < 1e-6);
    assert!((solution.certificate.primal - 1.0).abs() < 1e-5);
    assert!((solution.certificate.dual - 1.0).abs() < 1e-5);
    assert!(solution.certificate.dual_gradient_max <= 1.0);
    Ok(())
}

#[test]
fn conflicting_rows_use_slack_and_can_prefer_no_patch() -> Result<(), String> {
    let rows = [
        Row {
            target: true,
            activation: &[1.0],
        },
        Row {
            target: false,
            activation: &[1.0],
        },
    ];
    let solution = solve(&rows, controls())?;
    assert_eq!(solution.status, "no-op");
    assert!((solution.certificate.primal - 2.0).abs() < 1e-6);
    assert!((solution.certificate.dual - 2.0).abs() < 1e-5);
    Ok(())
}

#[test]
fn weak_target_penalty_prefers_zero_to_a_costly_edit() -> Result<(), String> {
    let rows = [Row {
        target: true,
        activation: &[1.0],
    }];
    let mut selected = controls();
    selected.target_penalty = 0.5;
    let solution = solve(&rows, selected)?;
    assert_eq!(solution.status, "no-op");
    assert!((solution.certificate.primal - 0.5).abs() < 1e-6);
    assert!(solution.certificate.relative_gap <= selected.relative_gap_tolerance);
    Ok(())
}

#[test]
fn zero_operator_reports_no_op_with_the_known_slack_objective() -> Result<(), String> {
    let rows = [Row {
        target: true,
        activation: &[0.0, 0.0],
    }];
    let solution = solve(&rows, controls())?;
    assert_eq!(solution.status, "no-op");
    assert!((solution.certificate.primal - 2.0).abs() < 1e-6);
    assert!((solution.certificate.dual - 2.0).abs() < 1e-5);
    Ok(())
}

#[test]
fn exhaustion_and_bad_inputs_never_become_certified_fits() -> Result<(), String> {
    let rows = [Row {
        target: true,
        activation: &[1.0],
    }];
    let mut selected = controls();
    selected.max_iterations = 1;
    assert_ne!(solve(&rows, selected)?.status, "converged");
    assert!(solve(&[], selected).is_err());
    assert!(
        solve(
            &[Row {
                target: true,
                activation: &[f32::NAN]
            }],
            selected
        )
        .is_err()
    );
    Ok(())
}
