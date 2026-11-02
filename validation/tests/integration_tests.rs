//! Integration tests using synthetic fixtures with known expected errors.
//! These test the comparator itself without requiring OpenFOAM or a Rust solver.

use foamcore_validation::compare::{compare_fields, gauge_align, CompareError, Tolerance};
use foamcore_validation::field::{Field, FieldType};
use foamcore_validation::report::{CaseReport, ComparisonEntry, TestStatus, ValidationReport};

/// Fixture: two identical scalar fields → should pass with zero error.
#[test]
fn fixture_identical_scalar() {
    let a = Field::scalar("phi", (0..100).map(|i| i as f64 * 0.1).collect());
    let b = a.clone();
    let tol = Tolerance { atol: 1e-14, rtol: 1e-12 };
    let result = compare_fields(&a, &b, &tol).unwrap();
    assert!(result.passed);
    assert_eq!(result.max_abs_error, 0.0);
    assert_eq!(result.rmse, 0.0);
}

/// Fixture: known perturbation of exactly 1e-9 on all values.
#[test]
fn fixture_known_perturbation() {
    let base: Vec<f64> = (0..50).map(|i| (i as f64) * 1.5).collect();
    let perturbed: Vec<f64> = base.iter().map(|v| v + 1e-9).collect();

    let a = Field::scalar("field", perturbed);
    let b = Field::scalar("field", base);
    let tol = Tolerance { atol: 1e-8, rtol: 1e-6 };

    let result = compare_fields(&a, &b, &tol).unwrap();
    assert!(result.passed);
    // Max abs error should be approximately 1e-9 (within FP representation noise)
    assert!(result.max_abs_error > 0.9e-9 && result.max_abs_error < 1.1e-9);
}

/// Fixture: deliberately altered value → must fail.
#[test]
fn fixture_deliberate_mismatch_fails() {
    let mut vals_a: Vec<f64> = (0..20).map(|i| i as f64).collect();
    let vals_b: Vec<f64> = (0..20).map(|i| i as f64).collect();

    // Alter one value significantly
    vals_a[7] += 0.5;

    let a = Field::scalar("field", vals_a);
    let b = Field::scalar("field", vals_b);
    let tol = Tolerance { atol: 1e-10, rtol: 1e-8 };

    let result = compare_fields(&a, &b, &tol).unwrap();
    assert!(!result.passed);
    assert_eq!(result.first_violation_index, Some(7));
    let (computed, ref_val) = result.first_violation_values.unwrap();
    assert!((computed - 7.5).abs() < 1e-14);
    assert!((ref_val - 7.0).abs() < 1e-14);
}

/// Fixture: vector field with known component-wise errors.
#[test]
fn fixture_vector_component_errors() {
    let n = 10;
    let x_ref = vec![1.0; n];
    let y_ref = vec![0.0; n];
    let z_ref = vec![0.0; n];

    // Perturb y-component by exactly 2e-9
    let x_comp = vec![1.0; n];
    let y_comp: Vec<f64> = (0..n).map(|_| 2e-9).collect();
    let z_comp = vec![0.0; n];

    let a = Field::vector("U", x_comp, y_comp, z_comp);
    let b = Field::vector("U", x_ref, y_ref, z_ref);
    let tol = Tolerance { atol: 1e-8, rtol: 1e-6 };

    let result = compare_fields(&a, &b, &tol).unwrap();
    assert!(result.passed);

    let comp_errs = result.max_component_errors.unwrap();
    assert!((comp_errs[0] - 0.0).abs() < 1e-15); // x: no error
    assert!((comp_errs[1] - 2e-9).abs() < 1e-15); // y: 2e-9
    assert!((comp_errs[2] - 0.0).abs() < 1e-15); // z: no error
}

/// Fixture: dimension mismatch → must produce CompareError, not panic.
#[test]
fn fixture_dimension_mismatch() {
    let a = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
    let b = Field::scalar("phi", vec![1.0, 2.0, 3.0, 4.0]);
    let tol = Tolerance::default();

    let result = compare_fields(&a, &b, &tol);
    match result {
        Err(CompareError::DimensionMismatch { a_n, b_n, .. }) => {
            assert_eq!(a_n, 3);
            assert_eq!(b_n, 4);
        }
        other => panic!("Expected DimensionMismatch, got {:?}", other.map(|r| r.passed)),
    }
}

/// Fixture: type mismatch (scalar vs vector) → must produce CompareError.
#[test]
fn fixture_type_mismatch() {
    let a = Field::scalar("field", vec![1.0, 2.0]);
    let b = Field::vector("field", vec![1.0, 2.0], vec![0.0; 2], vec![0.0; 2]);
    let tol = Tolerance::default();

    let result = compare_fields(&a, &b, &tol);
    assert!(matches!(result, Err(CompareError::TypeMismatch { .. })));
}

/// Fixture: NaN in computed field → must be detected as error.
#[test]
fn fixture_nan_detection() {
    let mut vals = vec![1.0, 2.0, 3.0];
    vals[1] = f64::NAN;
    let a = Field::scalar("phi", vals);
    let b = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
    let tol = Tolerance::default();

    let result = compare_fields(&a, &b, &tol);
    assert!(matches!(result, Err(CompareError::NonFiniteValues(_))));
}

/// Fixture: infinity detection.
#[test]
fn fixture_infinity_detection() {
    let a = Field::scalar("phi", vec![1.0, f64::INFINITY, 3.0]);
    let b = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
    let tol = Tolerance::default();

    let result = compare_fields(&a, &b, &tol);
    assert!(matches!(result, Err(CompareError::NonFiniteValues(_))));
}

/// Fixture: gauge alignment removes arbitrary constant offset.
#[test]
fn fixture_gauge_alignment() {
    let ref_vals: Vec<f64> = (0..20).map(|i| i as f64 * 0.5).collect();
    let offset = -123.456;
    let shifted: Vec<f64> = ref_vals.iter().map(|v| v + offset).collect();

    let mut computed = Field::scalar("Phi", shifted);
    let reference = Field::scalar("Phi", ref_vals);

    gauge_align(&mut computed, &reference);

    // After alignment, fields should be identical
    let tol = Tolerance { atol: 1e-12, rtol: 1e-10 };
    let result = compare_fields(&computed, &reference, &tol).unwrap();
    assert!(result.passed);
    assert!(result.max_abs_error < 1e-12);
}

/// Fixture: relative tolerance handles large values correctly.
#[test]
fn fixture_relative_tolerance_large_values() {
    // Values around 1e6 — absolute diff of 1.0 is fine with rtol=1e-5
    let a = Field::scalar("p", vec![1_000_000.0, 2_000_000.0]);
    let b = Field::scalar("p", vec![1_000_001.0, 1_999_999.0]);
    let tol = Tolerance { atol: 0.0, rtol: 1e-5 };

    let result = compare_fields(&a, &b, &tol).unwrap();
    assert!(result.passed);
    // 1.0 <= 0 + 1e-5 * 1e6 = 10.0 ✓
}

/// Fixture: mesh hash mismatch detection.
#[test]
fn fixture_mesh_hash_mismatch() {
    let mut a = Field::scalar("phi", vec![1.0, 2.0]);
    let mut b = Field::scalar("phi", vec![1.0, 2.0]);
    a.mesh_hash = Some("abc123".into());
    b.mesh_hash = Some("def456".into());
    let tol = Tolerance::default();

    let result = compare_fields(&a, &b, &tol);
    assert!(matches!(result, Err(CompareError::MeshMismatch)));
}

/// Test the full report generation pipeline with synthetic data.
#[test]
fn test_report_pipeline() {
    let field_a = Field::scalar("U", vec![1.0, 2.0, 3.0]);
    let field_b = Field::scalar("U", vec![1.0 + 1e-12, 2.0, 3.0 - 1e-12]);
    let tol = Tolerance { atol: 1e-10, rtol: 1e-8 };

    let cr = compare_fields(&field_a, &field_b, &tol).unwrap();
    assert!(cr.passed);

    let entry = ComparisonEntry {
        field_name: "U".into(),
        status: TestStatus::Pass,
        result: Some(cr),
        error_message: None,
        skip_reason: None,
    };

    let mut case_report = CaseReport {
        case_name: "synthetic_test".into(),
        solver: "test-solver".into(),
        solver_version: Some("0.0.1".into()),
        reference_source: "synthetic".into(),
        timestamp: "2024-01-01T00:00:00Z".into(),
        comparisons: vec![entry],
        overall_status: TestStatus::Skip,
        performance: None,
        summary: String::new(),
    };
    case_report.compute_overall();
    assert_eq!(case_report.overall_status, TestStatus::Pass);

    let vr = ValidationReport::new(vec![case_report]);
    assert!(vr.all_passed);

    // JSON serialization should work
    let json = serde_json::to_string(&vr).unwrap();
    assert!(json.contains("synthetic_test"));
}

/// Test that a missing reference produces ERROR not PASS or SKIP.
#[test]
fn test_missing_reference_is_error() {
    // Simulate what happens when reference file doesn't exist
    let entry = ComparisonEntry {
        field_name: "U".into(),
        status: TestStatus::Error,
        result: None,
        error_message: Some("Reference file not found".into()),
        skip_reason: None,
    };

    let mut report = CaseReport {
        case_name: "missing_ref".into(),
        solver: "foamcore".into(),
        solver_version: None,
        reference_source: "openfoam".into(),
        timestamp: "now".into(),
        comparisons: vec![entry],
        overall_status: TestStatus::Skip,
        performance: None,
        summary: String::new(),
    };
    report.compute_overall();

    // Must be Error, NOT Pass or Skip
    assert_eq!(report.overall_status, TestStatus::Error);
}
