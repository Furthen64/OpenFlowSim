use crate::compare::{compare_fields, compare_fields_rel_l2, ComparisonResult, Tolerance};
use crate::field::Field;
use foamcore_solver::mesh::MeshGeometry;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StepProfileResult {
    pub requested_x: f64,
    pub sampled_x: f64,
    pub relative_l2: f64,
    pub max_abs_error: f64,
    pub y: Vec<f64>,
    pub reference_ux: Vec<f64>,
    pub computed_ux: Vec<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StepRunDiagnostics {
    pub separation_x: Option<f64>,
    pub reattachment_x: Option<f64>,
    pub profiles: Vec<StepProfileResult>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StepFlowDiagnostics {
    pub reference: StepRunDiagnostics,
    pub computed: StepRunDiagnostics,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VortexPoint {
    pub x: f64,
    pub y: f64,
    pub streamfunction: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CavityFlowDiagnostics {
    pub streamfunction_relative_l2: f64,
    pub primary_reference: VortexPoint,
    pub primary_computed: VortexPoint,
    pub primary_center_distance: f64,
    pub reference_corner_extrema: usize,
    pub computed_corner_extrema: usize,
}

/// Reconstruct a gauge-fixed streamfunction on a structured orthogonal 2D mesh
/// using d(psi)/dy = Ux and d(psi)/dx = -Uy.
pub fn cavity_streamfunction(velocity: &Field, geometry: &MeshGeometry) -> Result<Field, String> {
    use std::collections::BTreeMap;
    if velocity.field_type != crate::field::FieldType::Vector
        || velocity.n_values != geometry.cells.len()
    {
        return Err("velocity field and mesh cell counts/types do not match".into());
    }
    let mut x_map = BTreeMap::<i64, f64>::new();
    let mut y_map = BTreeMap::<i64, f64>::new();
    for cell in &geometry.cells {
        x_map.insert((cell.centre.x * 1e9).round() as i64, cell.centre.x);
        y_map.insert((cell.centre.y * 1e9).round() as i64, cell.centre.y);
    }
    let x_keys: Vec<i64> = x_map.keys().copied().collect();
    let y_keys: Vec<i64> = y_map.keys().copied().collect();
    let xs: Vec<f64> = x_map.values().copied().collect();
    let ys: Vec<f64> = y_map.values().copied().collect();
    if xs.len() * ys.len() != geometry.cells.len() || xs.len() < 2 || ys.len() < 2 {
        return Err("cavity streamfunction requires a single structured 2D cell grid".into());
    }
    let mut grid = vec![vec![usize::MAX; xs.len()]; ys.len()];
    for (i, cell) in geometry.cells.iter().enumerate() {
        let ix = x_keys
            .binary_search(&((cell.centre.x * 1e9).round() as i64))
            .unwrap_or(usize::MAX);
        let iy = y_keys
            .binary_search(&((cell.centre.y * 1e9).round() as i64))
            .unwrap_or(usize::MAX);
        if ix >= xs.len() || iy >= ys.len() || grid[iy][ix] != usize::MAX {
            return Err("mesh cell centres do not form a structured cavity grid".into());
        }
        grid[iy][ix] = i;
    }
    if grid.iter().flatten().any(|i| *i == usize::MAX) {
        return Err("mesh cell centres do not form a complete cavity grid".into());
    }
    let uy = velocity
        .extra_components
        .first()
        .ok_or("U is missing its y component")?;
    let mut psi = vec![vec![0.0; xs.len()]; ys.len()];
    for ix in 1..xs.len() {
        let left = grid[0][ix - 1];
        let right = grid[0][ix];
        let dx = xs[ix] - xs[ix - 1];
        psi[0][ix] = psi[0][ix - 1] - 0.5 * (uy[left] + uy[right]) * dx;
    }
    for iy in 1..ys.len() {
        let dy = ys[iy] - ys[iy - 1];
        for ix in 0..xs.len() {
            let below = grid[iy - 1][ix];
            let above = grid[iy][ix];
            psi[iy][ix] =
                psi[iy - 1][ix] + 0.5 * (velocity.values[below] + velocity.values[above]) * dy;
        }
    }
    let mut values = vec![0.0; geometry.cells.len()];
    for iy in 0..ys.len() {
        for ix in 0..xs.len() {
            values[grid[iy][ix]] = psi[iy][ix];
        }
    }
    Ok(Field::scalar("psi", values))
}

/// Compare the cavity streamfunction field, main-vortex centre, and resolved
/// corner extrema between Rust and OpenFOAM velocity fields.
pub fn cavity_flow_diagnostics(
    computed_u: &Field,
    reference_u: &Field,
    geometry: &MeshGeometry,
) -> Result<CavityFlowDiagnostics, String> {
    let computed_psi = cavity_streamfunction(computed_u, geometry)?;
    let reference_psi = cavity_streamfunction(reference_u, geometry)?;
    let mut aligned = computed_psi.clone();
    crate::compare::gauge_align(&mut aligned, &reference_psi);
    let comparison = crate::compare::compare_fields_rel_l2(&aligned, &reference_psi, f64::MAX)
        .map_err(|e| e.to_string())?;
    let primary_reference = primary_vortex(&reference_psi, geometry);
    let primary_computed = primary_vortex(&computed_psi, geometry);
    let primary_center_distance =
        (primary_reference.x - primary_computed.x).hypot(primary_reference.y - primary_computed.y);
    Ok(CavityFlowDiagnostics {
        streamfunction_relative_l2: comparison.rel_l2_error,
        primary_reference,
        primary_computed,
        primary_center_distance,
        reference_corner_extrema: corner_extrema(&reference_psi, geometry),
        computed_corner_extrema: corner_extrema(&computed_psi, geometry),
    })
}

fn primary_vortex(streamfunction: &Field, geometry: &MeshGeometry) -> VortexPoint {
    let min_x = geometry
        .cells
        .iter()
        .map(|c| c.centre.x)
        .fold(f64::INFINITY, f64::min);
    let max_x = geometry
        .cells
        .iter()
        .map(|c| c.centre.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = geometry
        .cells
        .iter()
        .map(|c| c.centre.y)
        .fold(f64::INFINITY, f64::min);
    let max_y = geometry
        .cells
        .iter()
        .map(|c| c.centre.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let i = streamfunction
        .values
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            let c = geometry.cells[*i].centre;
            c.x > min_x + 0.1 * (max_x - min_x)
                && c.x < max_x - 0.1 * (max_x - min_x)
                && c.y > min_y + 0.1 * (max_y - min_y)
                && c.y < max_y - 0.1 * (max_y - min_y)
        })
        .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    VortexPoint {
        x: geometry.cells[i].centre.x,
        y: geometry.cells[i].centre.y,
        streamfunction: streamfunction.values[i],
    }
}

fn corner_extrema(streamfunction: &Field, geometry: &MeshGeometry) -> usize {
    let mut x_map = std::collections::BTreeMap::<i64, f64>::new();
    let mut y_map = std::collections::BTreeMap::<i64, f64>::new();
    for cell in &geometry.cells {
        x_map.insert((cell.centre.x * 1e9).round() as i64, cell.centre.x);
        y_map.insert((cell.centre.y * 1e9).round() as i64, cell.centre.y);
    }
    let x_keys: Vec<i64> = x_map.keys().copied().collect();
    let y_keys: Vec<i64> = y_map.keys().copied().collect();
    let xs: Vec<f64> = x_map.values().copied().collect();
    let ys: Vec<f64> = y_map.values().copied().collect();
    if xs.len() < 3 || ys.len() < 3 || xs.len() * ys.len() != geometry.cells.len() {
        return 0;
    }
    let mut grid = vec![vec![usize::MAX; xs.len()]; ys.len()];
    for (i, cell) in geometry.cells.iter().enumerate() {
        let ix = x_keys
            .binary_search(&((cell.centre.x * 1e9).round() as i64))
            .unwrap_or(usize::MAX);
        let iy = y_keys
            .binary_search(&((cell.centre.y * 1e9).round() as i64))
            .unwrap_or(usize::MAX);
        if ix >= xs.len() || iy >= ys.len() {
            continue;
        }
        grid[iy][ix] = i;
    }
    let width = xs[xs.len() - 1] - xs[0];
    let height = ys[ys.len() - 1] - ys[0];
    let mut count = 0;
    for iy in 1..ys.len() - 1 {
        for ix in 1..xs.len() - 1 {
            let i = grid[iy][ix];
            if i == usize::MAX {
                continue;
            }
            let value = streamfunction.values[i];
            let mut lower = true;
            let mut upper = true;
            for yj in iy - 1..=iy + 1 {
                for xi in ix - 1..=ix + 1 {
                    if yj == iy && xi == ix {
                        continue;
                    }
                    let other = grid[yj][xi];
                    if other == usize::MAX {
                        lower = false;
                        upper = false;
                        continue;
                    }
                    lower &= value < streamfunction.values[other];
                    upper &= value > streamfunction.values[other];
                }
            }
            let x = xs[ix];
            let y = ys[iy];
            let near_corner = (x - xs[0] < 0.2 * width || xs[xs.len() - 1] - x < 0.2 * width)
                && (y - ys[0] < 0.2 * height || ys[ys.len() - 1] - y < 0.2 * height);
            if near_corner && (lower || upper) {
                count += 1;
            }
        }
    }
    count
}

/// Compare near-wall separation/reattachment and streamwise velocity profiles
/// for a 2D backward-facing-step case on the same cell-centred mesh.
pub fn backward_facing_step_diagnostics(
    computed_u: &Field,
    reference_u: &Field,
    geometry: &MeshGeometry,
    step_x: f64,
    stations: &[f64],
) -> Result<StepFlowDiagnostics, String> {
    for field in [computed_u, reference_u] {
        if field.field_type != crate::field::FieldType::Vector
            || field.n_values != geometry.cells.len()
        {
            return Err("velocity field and mesh cell counts/types do not match".into());
        }
    }
    Ok(StepFlowDiagnostics {
        reference: step_run_diagnostics(reference_u, None, geometry, step_x, stations),
        computed: step_run_diagnostics(computed_u, Some(reference_u), geometry, step_x, stations),
    })
}

fn step_run_diagnostics(
    velocity: &Field,
    compare_to: Option<&Field>,
    geometry: &MeshGeometry,
    step_x: f64,
    stations: &[f64],
) -> StepRunDiagnostics {
    use std::collections::BTreeMap;

    let mut columns: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, cell) in geometry.cells.iter().enumerate() {
        columns
            .entry((cell.centre.x * 1.0e9).round() as i64)
            .or_default()
            .push(i);
    }
    let mut wall_line: Vec<(f64, usize)> = columns
        .values()
        .filter_map(|indices| {
            let x = geometry.cells[indices[0]].centre.x;
            if x <= step_x {
                return None;
            }
            indices
                .iter()
                .copied()
                .min_by(|a, b| {
                    geometry.cells[*a]
                        .centre
                        .y
                        .total_cmp(&geometry.cells[*b].centre.y)
                })
                .map(|i| (x, i))
        })
        .collect();
    wall_line.sort_by(|a, b| a.0.total_cmp(&b.0));
    let negative: Vec<usize> = wall_line
        .iter()
        .enumerate()
        .filter_map(|(j, (_, i))| (velocity.values[*i] < 0.0).then_some(j))
        .collect();
    let separation_x = negative.first().map(|_| step_x);
    let reattachment_x = negative.last().and_then(|last| {
        wall_line
            .iter()
            .skip(last + 1)
            .find(|(_, i)| velocity.values[*i] >= 0.0)
            .map(|(x, _)| *x)
    });

    let mut profiles = Vec::new();
    for &requested_x in stations {
        let Some(compare_to) = compare_to else {
            break;
        };
        let Some((sampled_x, indices)) = columns
            .values()
            .map(|indices| (geometry.cells[indices[0]].centre.x, indices))
            .filter(|(x, _)| *x > step_x)
            .min_by(|(a, _), (b, _)| (a - requested_x).abs().total_cmp(&(b - requested_x).abs()))
        else {
            continue;
        };
        let mut indices = indices.clone();
        indices.sort_by(|a, b| {
            geometry.cells[*a]
                .centre
                .y
                .total_cmp(&geometry.cells[*b].centre.y)
        });
        let mut norm_error_sq = 0.0;
        let mut norm_reference_sq = 0.0;
        let mut max_abs_error: f64 = 0.0;
        let mut y = Vec::with_capacity(indices.len());
        let mut reference_ux = Vec::with_capacity(indices.len());
        let mut computed_ux = Vec::with_capacity(indices.len());
        for &i in &indices {
            let error = velocity.values[i] - compare_to.values[i];
            norm_error_sq += error * error;
            norm_reference_sq += compare_to.values[i] * compare_to.values[i];
            max_abs_error = max_abs_error.max(error.abs());
            y.push(geometry.cells[i].centre.y);
            reference_ux.push(compare_to.values[i]);
            computed_ux.push(velocity.values[i]);
        }
        profiles.push(StepProfileResult {
            requested_x,
            sampled_x,
            relative_l2: norm_error_sq.sqrt() / norm_reference_sq.sqrt().max(f64::MIN_POSITIVE),
            max_abs_error,
            y,
            reference_ux,
            computed_ux,
        });
    }
    StepRunDiagnostics {
        separation_x,
        reattachment_x,
        profiles,
    }
}

/// Check the outlet section of a low-Reynolds-number plane channel against
/// the fully developed parallel-plate Poiseuille profile.
pub fn verify_poiseuille_outlet(
    computed_u: &Field,
    geometry: &MeshGeometry,
    mean_velocity: f64,
    channel_height: f64,
    outlet_x_min: f64,
    max_relative_l2: f64,
) -> Result<ComparisonResult, String> {
    if computed_u.field_type != crate::field::FieldType::Vector
        || computed_u.n_values != geometry.cells.len()
    {
        return Err("velocity field and mesh cell counts/types do not match".into());
    }
    let mut computed = Vec::new();
    let mut exact = Vec::new();
    for (i, cell) in geometry.cells.iter().enumerate() {
        if cell.centre.x >= outlet_x_min {
            let y = cell.centre.y / channel_height;
            computed.push(computed_u.values[i]);
            exact.push(6.0 * mean_velocity * y * (1.0 - y));
        }
    }
    if computed.is_empty() {
        return Err("no cells found in the requested outlet section".into());
    }
    compare_fields_rel_l2(
        &Field::scalar("U_outlet_x", computed),
        &Field::scalar("U_poiseuille_x", exact),
        max_relative_l2,
    )
    .map_err(|e| e.to_string())
}

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
            sorted_exact
                .values
                .sort_by(|a, b| a.partial_cmp(b).unwrap());
            Some(compare_fields(&aligned_cp, &sorted_exact, tol_phi))
        } else {
            None
        };

        let u_ok = u_result.as_ref().ok();
        let phi_ok = phi_result.as_ref().and_then(|r| r.as_ref().ok());

        let all_passed =
            u_ok.map(|r| r.passed).unwrap_or(false) && phi_ok.map(|r| r.passed).unwrap_or(true);

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
            writeln!(
                f,
                "  U:   max_abs={:.3e}, rmse={:.3e}",
                r.max_abs_error, r.rmse
            )?;
        }
        if let Some(r) = &self.phi_comparison {
            writeln!(
                f,
                "  Phi: max_abs={:.3e}, rmse={:.3e}",
                r.max_abs_error, r.rmse
            )?;
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
    use foamcore_solver::mesh::{CellGeometry, Vector3};

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
            assert!(
                (got - exp).abs() < 1e-14,
                "index {}: got {}, expected {}",
                i,
                got,
                exp
            );
        }
    }

    #[test]
    fn test_perfect_solution_passes() {
        let case = UniformChannelAnalytical::new(1.0, 2.0, 10, 5);
        let u_exact = case.exact_u();
        let phi_exact = case.exact_phi();

        let tol_u = Tolerance {
            atol: 1e-12,
            rtol: 1e-10,
        };
        let tol_p = Tolerance {
            atol: 1e-12,
            rtol: 1e-10,
        };

        let report = case.verify(&u_exact, Some(&phi_exact), &tol_u, &tol_p);
        assert!(report.all_passed);
    }

    #[test]
    fn test_wrong_velocity_fails() {
        let case = UniformChannelAnalytical::new(1.0, 2.0, 10, 5);
        // Computed U has wrong magnitude
        let n = case.nx * case.ny;
        let bad_u = Field::vector("U", vec![2.0; n], vec![0.0; n], vec![0.0; n]);

        let tol_u = Tolerance {
            atol: 1e-12,
            rtol: 1e-10,
        };
        let tol_p = Tolerance {
            atol: 1e-12,
            rtol: 1e-10,
        };

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

        let tol_u = Tolerance {
            atol: 1e-12,
            rtol: 1e-10,
        };
        let tol_p = Tolerance {
            atol: 1e-12,
            rtol: 1e-10,
        };

        let u_exact = case.exact_u();
        let report = case.verify(&u_exact, Some(&phi_shifted), &tol_u, &tol_p);
        assert!(
            report.all_passed,
            "Gauge-shifted Phi should still pass after alignment"
        );
    }

    #[test]
    fn poiseuille_outlet_matches_parallel_plate_profile() {
        let geometry = MeshGeometry {
            faces: Vec::new(),
            cells: vec![
                CellGeometry {
                    centre: Vector3 {
                        x: 1.6,
                        y: 0.25,
                        z: 0.0,
                    },
                    volume: 1.0,
                },
                CellGeometry {
                    centre: Vector3 {
                        x: 1.6,
                        y: 0.75,
                        z: 0.0,
                    },
                    volume: 1.0,
                },
                CellGeometry {
                    centre: Vector3 {
                        x: 1.0,
                        y: 0.5,
                        z: 0.0,
                    },
                    volume: 1.0,
                },
            ],
        };
        let velocity = Field::vector("U", vec![1.125, 1.125, 1.0], vec![0.0; 3], vec![0.0; 3]);
        let result = verify_poiseuille_outlet(&velocity, &geometry, 1.0, 1.0, 1.5, 1e-12).unwrap();
        assert!(result.passed);
        assert_eq!(result.n_values, 2);
    }

    #[test]
    fn backward_step_diagnostics_find_reattachment_and_compare_profiles() {
        let mut cells = Vec::new();
        for x in [0.25, 0.5, 0.75] {
            for y in [-0.25, 0.25] {
                cells.push(CellGeometry {
                    centre: Vector3 { x, y, z: 0.0 },
                    volume: 1.0,
                });
            }
        }
        let geometry = MeshGeometry {
            faces: Vec::new(),
            cells,
        };
        let reference = Field::vector(
            "U",
            vec![-1.0, 2.0, -0.5, 2.0, 1.0, 2.0],
            vec![0.0; 6],
            vec![0.0; 6],
        );
        let computed = reference.clone();
        let report =
            backward_facing_step_diagnostics(&computed, &reference, &geometry, 0.0, &[0.5])
                .unwrap();
        assert_eq!(report.reference.separation_x, Some(0.0));
        assert_eq!(report.reference.reattachment_x, Some(0.75));
        assert_eq!(report.computed.profiles[0].relative_l2, 0.0);
        assert_eq!(report.computed.profiles[0].y, vec![-0.25, 0.25]);
    }

    #[test]
    fn cavity_streamfunction_integrates_uniform_vertical_velocity() {
        let mut cells = Vec::new();
        for y in [0.25, 0.75] {
            for x in [0.25, 0.75] {
                cells.push(CellGeometry {
                    centre: Vector3 { x, y, z: 0.0 },
                    volume: 1.0,
                });
            }
        }
        let geometry = MeshGeometry {
            faces: Vec::new(),
            cells,
        };
        let velocity = Field::vector("U", vec![1.0; 4], vec![0.0; 4], vec![0.0; 4]);
        let psi = cavity_streamfunction(&velocity, &geometry).unwrap();
        for (i, cell) in geometry.cells.iter().enumerate() {
            assert!((psi.values[i] - (cell.centre.y - 0.25)).abs() < 1e-12);
        }
    }
}
