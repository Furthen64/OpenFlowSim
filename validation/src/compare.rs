use serde::{Deserialize, Serialize};
use std::fmt;

use crate::field::{Field, FieldType};

/// Tolerance configuration for a single field comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tolerance {
    /// Absolute tolerance.
    pub atol: f64,
    /// Relative tolerance (applied to |reference|).
    pub rtol: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            atol: 1e-10,
            rtol: 1e-8,
        }
    }
}

impl Tolerance {
    /// Check if a pair of values is within tolerance.
    pub fn passes(&self, computed: f64, reference: f64) -> bool {
        let diff = (computed - reference).abs();
        diff <= self.atol + self.rtol * reference.abs()
    }
}

/// Result of comparing two fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    /// Name of the field compared.
    pub field_name: String,
    /// Field type.
    pub field_type: FieldType,
    /// Number of values compared.
    pub n_values: usize,
    /// Maximum absolute error across all components.
    pub max_abs_error: f64,
    /// Root-mean-square error.
    pub rmse: f64,
    /// Relative L2 error: ||a-b||_2 / ||b||_2.
    pub rel_l2_error: f64,
    /// For vectors: max component-wise error per component [x, y, z].
    #[serde(default)]
    pub max_component_errors: Option<Vec<f64>>,
    /// Whether any NaN or Inf was detected in either field.
    pub has_non_finite: bool,
    /// Index of first tolerance violation (if any).
    pub first_violation_index: Option<usize>,
    /// Value at first violation (computed, reference).
    pub first_violation_values: Option<(f64, f64)>,
    /// Overall pass/fail.
    pub passed: bool,
    /// Tolerance used.
    pub tolerance: Tolerance,
}

impl fmt::Display for ComparisonResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.passed { "PASS" } else { "FAIL" };
        write!(
            f,
            "[{status}] {}: max_abs={:.3e}, rmse={:.3e}, rel_l2={:.3e} (n={})",
            self.field_name, self.max_abs_error, self.rmse, self.rel_l2_error, self.n_values
        )?;
        if !self.passed {
            if let Some(idx) = self.first_violation_index {
                let (c, r) = self.first_violation_values.unwrap_or((0.0, 0.0));
                write!(
                    f,
                    "\n  first violation at index {idx}: computed={c:.12e}, ref={r:.12e}"
                )?;
            }
        }
        Ok(())
    }
}

/// Errors during comparison that indicate data integrity issues (not numerical mismatch).
#[derive(Debug, thiserror::Error)]
pub enum CompareError {
    #[error("Field dimension mismatch: '{a}' has {} values, '{b}' has {}", .a_n, .b_n)]
    DimensionMismatch {
        a: String,
        b: String,
        a_n: usize,
        b_n: usize,
    },
    #[error("Field type mismatch: '{a}' is {:?}, '{b}' is {:?}", .a.field_type, .b.field_type)]
    TypeMismatch { a: Field, b: Field },
    #[error("Mesh hash mismatch: fields were computed on different meshes")]
    MeshMismatch,
    #[error("Non-finite values detected in field '{}'", .0)]
    NonFiniteValues(String),
}

/// Compare two scalar or vector fields element-wise.
///
/// Returns `Ok(ComparisonResult)` for numerical pass/fail,
/// or `Err(CompareError)` for structural/data-integrity failures.
pub fn compare_fields(
    a: &Field,
    b: &Field,
    tol: &Tolerance,
) -> Result<ComparisonResult, CompareError> {
    // Structural checks first
    if a.n_values != b.n_values {
        return Err(CompareError::DimensionMismatch {
            a: a.name.clone(),
            b: b.name.clone(),
            a_n: a.n_values,
            b_n: b.n_values,
        });
    }

    if a.field_type != b.field_type {
        return Err(CompareError::TypeMismatch {
            a: a.clone(),
            b: b.clone(),
        });
    }

    if let (Some(ha), Some(hb)) = (&a.mesh_hash, &b.mesh_hash) {
        if ha != hb {
            return Err(CompareError::MeshMismatch);
        }
    }

    if a.has_non_finite() || b.has_non_finite() {
        return Err(CompareError::NonFiniteValues(format!(
            "{} (in {})",
            if a.has_non_finite() {
                "computed"
            } else {
                "reference"
            },
            a.name
        )));
    }

    let n = a.n_values;
    let n_comp = a.n_components();

    let mut max_abs: f64 = 0.0;
    let mut sum_sq: f64 = 0.0;
    let mut sum_ref_sq: f64 = 0.0;
    let mut comp_max: Vec<f64> = vec![0.0; n_comp];
    let mut first_violation: Option<(usize, f64, f64)> = None;

    for i in 0..n {
        for c in 0..n_comp {
            let va = get_component(a, i, c);
            let vb = get_component(b, i, c);

            let diff = (va - vb).abs();
            sum_sq += diff * diff;
            sum_ref_sq += vb * vb;

            if diff > comp_max[c] {
                comp_max[c] = diff;
            }
            if diff > max_abs {
                max_abs = diff;
            }

            if !tol.passes(va, vb) && first_violation.is_none() {
                first_violation = Some((i, va, vb));
            }
        }
    }

    let rmse = (sum_sq / (n as f64 * n_comp as f64)).sqrt();
    let rel_l2 = if sum_ref_sq > 0.0 {
        (sum_sq / sum_ref_sq).sqrt()
    } else {
        // Reference is all zeros — use absolute measure
        if sum_sq > 0.0 {
            f64::INFINITY
        } else {
            0.0
        }
    };

    let passed = first_violation.is_none();

    Ok(ComparisonResult {
        field_name: a.name.clone(),
        field_type: a.field_type,
        n_values: n,
        max_abs_error: max_abs,
        rmse,
        rel_l2_error: rel_l2,
        max_component_errors: if a.field_type == FieldType::Vector {
            Some(comp_max)
        } else {
            None
        },
        has_non_finite: false,
        first_violation_index: first_violation.as_ref().map(|(i, _, _)| *i),
        first_violation_values: first_violation.as_ref().map(|(_, c, r)| (*c, *r)),
        passed,
        tolerance: tol.clone(),
    })
}

/// Compare fields using a relative L2 threshold, as used by CFD validation
/// targets that specify a norm rather than a pointwise tolerance.
pub fn compare_fields_rel_l2(
    a: &Field,
    b: &Field,
    max_relative_l2: f64,
) -> Result<ComparisonResult, CompareError> {
    let mut result = compare_fields(
        a,
        b,
        &Tolerance {
            atol: f64::MAX,
            rtol: 0.0,
        },
    )?;
    result.passed = result.rel_l2_error <= max_relative_l2;
    result.tolerance = Tolerance {
        atol: 0.0,
        rtol: max_relative_l2,
    };
    Ok(result)
}

/// Get component `c` of field at index `i`.
fn get_component(field: &Field, i: usize, c: usize) -> f64 {
    match field.field_type {
        FieldType::Scalar => field.values[i],
        FieldType::Vector => match c {
            0 => field.values[i],
            1 => field.extra_components[0][i],
            2 => field.extra_components[1][i],
            _ => unreachable!(),
        },
    }
}

/// Gauge-align two scalar fields by subtracting the mean of (a - b) from a.
/// Used for potentials defined only up to an additive constant.
pub fn gauge_align(a: &mut Field, b: &Field) {
    if a.n_values != b.n_values || a.n_values == 0 {
        return;
    }
    let offset: f64 = a
        .values
        .iter()
        .zip(b.values.iter())
        .map(|(x, y)| x - y)
        .sum::<f64>()
        / a.n_values as f64;
    for i in 0..a.n_values {
        a.values[i] -= offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_fields_pass() {
        let a = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
        let b = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
        let tol = Tolerance {
            atol: 1e-10,
            rtol: 1e-8,
        };
        let result = compare_fields(&a, &b, &tol).unwrap();
        assert!(result.passed);
        assert_eq!(result.max_abs_error, 0.0);
    }

    #[test]
    fn test_small_difference_passes() {
        let a = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
        let b = Field::scalar("phi", vec![1.0 + 1e-12, 2.0 - 1e-12, 3.0 + 1e-12]);
        let tol = Tolerance {
            atol: 1e-10,
            rtol: 1e-8,
        };
        let result = compare_fields(&a, &b, &tol).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_large_difference_fails() {
        let a = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
        let b = Field::scalar("phi", vec![1.0, 2.0, 5.0]);
        let tol = Tolerance {
            atol: 1e-10,
            rtol: 1e-8,
        };
        let result = compare_fields(&a, &b, &tol).unwrap();
        assert!(!result.passed);
        assert_eq!(result.first_violation_index, Some(2));
    }

    #[test]
    fn test_dimension_mismatch_errors() {
        let a = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
        let b = Field::scalar("phi", vec![1.0, 2.0]);
        let tol = Tolerance::default();
        let result = compare_fields(&a, &b, &tol);
        assert!(matches!(
            result,
            Err(CompareError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn test_nan_detection() {
        let a = Field::scalar("phi", vec![1.0, f64::NAN, 3.0]);
        let b = Field::scalar("phi", vec![1.0, 2.0, 3.0]);
        let tol = Tolerance::default();
        let result = compare_fields(&a, &b, &tol);
        assert!(matches!(result, Err(CompareError::NonFiniteValues(_))));
    }

    #[test]
    fn test_vector_field_comparison() {
        let a = Field::vector("U", vec![1.0, 2.0], vec![0.0, 0.0], vec![0.0, 0.0]);
        let b = Field::vector("U", vec![1.0 + 1e-9, 2.0], vec![1e-11, 0.0], vec![0.0, 0.0]);
        let tol = Tolerance {
            atol: 1e-8,
            rtol: 1e-6,
        };
        let result = compare_fields(&a, &b, &tol).unwrap();
        assert!(result.passed);
        assert!(result.max_component_errors.is_some());
    }

    #[test]
    fn test_gauge_alignment() {
        let mut a = Field::scalar("Phi", vec![1.0, 2.0, 3.0]);
        let b = Field::scalar("Phi", vec![4.0, 5.0, 6.0]);
        // Offset is 3.0
        gauge_align(&mut a, &b);
        assert!((a.values[0] - 4.0).abs() < 1e-14);
        assert!((a.values[1] - 5.0).abs() < 1e-14);
        assert!((a.values[2] - 6.0).abs() < 1e-14);
    }

    #[test]
    fn test_relative_tolerance_near_zero() {
        // Near zero, absolute tolerance dominates
        let a = Field::scalar("phi", vec![1e-15]);
        let b = Field::scalar("phi", vec![0.0]);
        let tol = Tolerance {
            atol: 1e-10,
            rtol: 1e-8,
        };
        let result = compare_fields(&a, &b, &tol).unwrap();
        assert!(result.passed); // 1e-15 < 1e-10
    }

    #[test]
    fn test_rmse_computation() {
        let a = Field::scalar("phi", vec![0.0, 0.0]);
        let b = Field::scalar("phi", vec![1.0, -1.0]);
        let tol = Tolerance {
            atol: 1.0,
            rtol: 1.0,
        };
        let result = compare_fields(&a, &b, &tol).unwrap();
        // RMSE = sqrt((1+1)/2) = 1.0
        assert!((result.rmse - 1.0).abs() < 1e-14);
    }

    #[test]
    fn relative_l2_comparison_uses_norm_threshold() {
        let reference = Field::scalar("p", vec![1.0, 2.0, 3.0]);
        let computed = Field::scalar("p", vec![1.001, 2.0, 3.0]);
        let result = compare_fields_rel_l2(&computed, &reference, 1e-3).unwrap();
        assert!(result.passed);
        assert!(result.rel_l2_error < 1e-3);
    }
}
