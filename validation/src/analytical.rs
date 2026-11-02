use crate::compare::{compare_fields, ComparisonResult, Tolerance};
use crate::field::Field;

/// Analytical verification case: uniform flow through a straight channel.
///
/// For incompressible potential flow in a straight rectangular channel with
/// no obstacles, the exact solution is:
///   U(x, y, z) = (U0, 0, 0)  everywhere
///   Phi(x, y, z) = U0 * x + C
///
/// This is independent of any CFD code — it follows directly from:
///   - Continuity: div(U) = 0 satisfied by constant U
///   - Irrotationality: curl(U) = 0 satisfied by constant U
///   - Boundary conditions: inlet fixes U_x = U0, walls are symmetry (zero normal velocity)
pub struct UniformChannelAnalytical {
    /// Inlet velocity magnitude.
    pub u0: f64,
    /// Channel length in x.
    pub length: f64,
    /// Number of cells in x direction.
    pub nx: usize,
    /// Number of cells in y direction.
    pub ny: usize,
}

impl UniformChannelAnalytical {
    pub fn new(u0: f64, length: f64, nx: usize, ny: usize) -> Self {
        Self { u0, length, nx, ny }
    }

    /// Generate the analytical U field (cell-centered).
    /// Cell centers are at (x_i, y_j) where x_i = (i + 0.5) * dx.
    pub fn exact_u(&self) -> Field {
        let n = self.nx * self.ny;
        let mut ux = Vec::with_capacity(n);
        let mut uy = Vec::with_capacity(n);
        let mut uz = Vec::with_capacity(n);

        for _j in 0..self.ny {
            for _i in 0..self.nx {
                ux.push(self.u0);
                uy.push(0.0);
                uz.push(0.0);
            }
        }

        Field::vector("U", ux, uy, uz)
    }

    /// Generate the analytical Phi field (cell-centered), gauge-aligned so Phi=0 at x=0.
    /// Cell center x-coordinate: x_i = (i + 0.5) * dx
    pub fn exact_phi(&self) -> Field {
        let dx = self.length / self.nx as f64;
        let n = self.nx * self.ny;
        let mut values = Vec::with_capacity(n);

        for _j in 0..self.ny {
            for i in 0..self.nx {
                let x_center = (i as f64 + 0.5) * dx;
                values.push(self.u0 * x_center);
            }
        }

        Field::scalar("Phi", values)
    }

    /// Run the full analytical verification against a computed result.
    /// Returns comparison results for U and Phi.
    pub fn verify(
        &self,
        computed_u: &Field,
        computed_phi: Option<&Field>,
        tol_u: &Tolerance,
        tol_phi: &Tolerance,
    ) -> AnalyticalReport {
        let exact_u = self.exact_u();
        let u_result = compare_fields(computed_u, &exact_u, tol_u);

        let phi_result = if let Some(cp) = computed_phi {
            let exact_p = self.exact_phi();
            // For Phi we cannot assume cell ordering matches between implementations.
            // Compare as sorted multisets after gauge alignment.
            let mut aligned_cp = cp.clone();
            crate::compare::gauge_align(&mut aligned_cp, &exact_p);
            // Sort both by value for order-independent comparison
            aligned_cp.values.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut sorted_exact = exact_p;
            sorted_exact.values.sort_by(|a, b| a.partial_cmp(b).unwrap());
            Some(compare_fields(&aligned_cp, &sorted_exact, tol_phi))
        } else {
            None
        };

        let u_ok = u_result.as_ref().ok();
        let phi_ok = phi_result.as_ref().and_then(|r| r.as_ref().ok());

        let all_passed = u_ok.map(|r| r.passed).unwrap_or(false)
            && phi_ok.map(|r| r.passed).unwrap_or(true);

        AnalyticalReport {
            case_name: "simpleChannel_analytical".to_string(),
            description: format!(
                "Uniform flow U0={:.4} in channel L={:.2}, {}x{} cells",
                self.u0, self.length, self.nx, self.ny
            ),
            u_comparison: u_result.ok(),
            phi_comparison: phi_result.and_then(|r| r.ok()),
            phi_missing: computed_phi.is_none(),
            all_passed,
        }
    }
}

/// Report from an analytical verification run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyticalReport {
    pub case_name: String,
    pub description: String,
    pub u_comparison: Option<ComparisonResult>,
    pub phi_comparison: Option<ComparisonResult>,
    pub phi_missing: bool,
    pub all_passed: bool,
}

impl std::fmt::Display for AnalyticalReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.all_passed { "PASS" } else { "FAIL" };
        writeln!(f, "[{status}] Analytical: {}", self.description)?;
        if let Some(r) = &self.u_comparison {
            writeln!(f, "  U:   max_abs={:.3e}, rmse={:.3e}", r.max_abs_error, r.rmse)?;
        }
        if let Some(r) = &self.phi_comparison {
            writeln!(f, "  Phi: max_abs={:.3e}, rmse={:.3e}", r.max_abs_error, r.rmse)?;
        }
        if self.phi_missing {
            writeln!(f, "  Phi: not available (skipped)")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analytical_u_is_uniform() {
        let case = UniformChannelAnalytical::new(1.0, 2.0, 4, 2);
        let u = case.exact_u();
        assert_eq!(u.n_values, 8);
        // All x-components should be 1.0
        for v in &u.values {
            assert!((v - 1.0).abs() < 1e-15);
        }
        // All y,z components should be 0.0
        for v in &u.extra_components[0] {
            assert!(v.abs() < 1e-15);
        }
    }

    #[test]
    fn test_analytical_phi_is_linear_in_x() {
        let case = UniformChannelAnalytical::new(2.0, 4.0, 4, 1);
        let phi = case.exact_phi();
        // dx = 1.0, centers at 0.5, 1.5, 2.5, 3.5
        // Phi = 2*x => 1.0, 3.0, 5.0, 7.0
        let expected = vec![1.0, 3.0, 5.0, 7.0];
        for (i, (got, exp)) in phi.values.iter().zip(&expected).enumerate() {
            assert!((got - exp).abs() < 1e-14, "index {}: got {}, expected {}", i, got, exp);
        }
    }

    #[test]
    fn test_perfect_solution_passes() {
        let case = UniformChannelAnalytical::new(1.0, 2.0, 10, 5);
        let u_exact = case.exact_u();
        let phi_exact = case.exact_phi();

        let tol_u = Tolerance { atol: 1e-12, rtol: 1e-10 };
        let tol_p = Tolerance { atol: 1e-12, rtol: 1e-10 };

        let report = case.verify(&u_exact, Some(&phi_exact), &tol_u, &tol_p);
        assert!(report.all_passed);
    }

    #[test]
    fn test_wrong_velocity_fails() {
        let case = UniformChannelAnalytical::new(1.0, 2.0, 10, 5);
        // Computed U has wrong magnitude
        let n = case.nx * case.ny;
        let bad_u = Field::vector("U", vec![2.0; n], vec![0.0; n], vec![0.0; n]);

        let tol_u = Tolerance { atol: 1e-12, rtol: 1e-10 };
        let tol_p = Tolerance { atol: 1e-12, rtol: 1e-10 };

        let report = case.verify(&bad_u, None, &tol_u, &tol_p);
        assert!(!report.all_passed);
    }

    #[test]
    fn test_gauge_invariance_of_phi() {
        let case = UniformChannelAnalytical::new(1.0, 2.0, 10, 5);
        let phi_exact = case.exact_phi();

        // Add arbitrary constant to computed Phi
        let offset = 42.7;
        let mut phi_shifted = phi_exact.clone();
        for v in phi_shifted.values.iter_mut() {
            *v += offset;
        }

        let tol_u = Tolerance { atol: 1e-12, rtol: 1e-10 };
        let tol_p = Tolerance { atol: 1e-12, rtol: 1e-10 };

        let u_exact = case.exact_u();
        let report = case.verify(&u_exact, Some(&phi_shifted), &tol_u, &tol_p);
        assert!(report.all_passed, "Gauge-shifted Phi should still pass after alignment");
    }
}
