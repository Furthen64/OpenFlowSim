//! Sparse finite-volume matrix with conjugate-gradient solver.
//!
//! Uses the same storage layout as `solver/src/flow.rs`:
//! per-cell diagonal plus a list of (neighbour, coefficient) off-diagonals.

use crate::{MeshGeometry, PolyMesh, Vector3};

#[derive(Debug, thiserror::Error)]
pub enum SolveError {
    #[error("linear solver failed to converge after {iterations} iterations (residual {residual:.4e})")]
    NoConvergence { residual: f64, iterations: usize },
}

/// A sparse matrix assembled from FV operators, ready for solution.
#[derive(Debug, Clone)]
pub struct FvMatrix {
    pub n_cells: usize,
    pub diagonal: Vec<f64>,
    pub off_diagonal: Vec<Vec<(usize, f64)>>,
    pub source: Vec<f64>,
}

impl FvMatrix {
    pub fn new(n_cells: usize) -> Self {
        Self {
            n_cells,
            diagonal: vec![0.0; n_cells],
            off_diagonal: vec![Vec::new(); n_cells],
            source: vec![0.0; n_cells],
        }
    }

    /// Add a value to the diagonal of cell `i`.
    pub fn add_diagonal(&mut self, i: usize, val: f64) {
        self.diagonal[i] += val;
    }

    /// Add an off-diagonal coupling between cells `i` and `j`.
    pub fn add_off_diagonal(&mut self, i: usize, j: usize, val: f64) {
        self.off_diagonal[i].push((j, val));
    }

    /// Add to the source (RHS) vector at cell `i`.
    pub fn add_source(&mut self, i: usize, val: f64) {
        self.source[i] += val;
    }

    /// Solve A*x = source using conjugate gradient with diagonal preconditioning.
    pub fn solve(&self, mut x: Vec<f64>) -> Result<Vec<f64>, SolveError> {
        let n = self.n_cells;
        let rhs_norm = dot(&self.source, &self.source).sqrt().max(1.0);

        let apply = |x: &[f64], out: &mut [f64]| {
            for i in 0..n {
                out[i] = self.diagonal[i] * x[i]
                    + self.off_diagonal[i].iter().map(|&(j, a)| a * x[j]).sum::<f64>();
            }
        };

        let mut r = vec![0.0; n];
        let mut p = vec![0.0; n];
        apply(&x, &mut p);
        for i in 0..n {
            r[i] = self.source[i] - p[i];
        }

        let initial_norm = dot(&r, &r).sqrt();
        if initial_norm <= 1.0e-12 * rhs_norm {
            return Ok(x);
        }

        // Diagonal preconditioner
        let mut z = vec![0.0; n];
        for i in 0..n {
            z[i] = if self.diagonal[i].abs() > f64::MIN_POSITIVE {
                r[i] / self.diagonal[i]
            } else {
                r[i]
            };
        }
        let mut d = z.clone();
        let mut rz = dot(&r, &z);

        let max_iter = (n.saturating_mul(20)).max(500);
        for _iter in 0..max_iter {
            let mut ad = vec![0.0; n];
            apply(&d, &mut ad);
            let d_ad = dot(&d, &ad);
            if d_ad.abs() < f64::MIN_POSITIVE {
                break;
            }
            let alpha = rz / d_ad;
            for i in 0..n {
                x[i] += alpha * d[i];
                r[i] -= alpha * ad[i];
            }
            if dot(&r, &r).sqrt() <= 1.0e-10 * rhs_norm {
                return Ok(x);
            }
            for i in 0..n {
                z[i] = if self.diagonal[i].abs() > f64::MIN_POSITIVE {
                    r[i] / self.diagonal[i]
                } else {
                    r[i]
                };
            }
            let new_rz = dot(&r, &z);
            let beta = new_rz / rz;
            for i in 0..n {
                d[i] = z[i] + beta * d[i];
            }
            rz = new_rz;
        }

        Err(SolveError::NoConvergence {
            residual: dot(&r, &r).sqrt(),
            iterations: max_iter,
        })
    }

    /// Solve with zero initial guess.
    pub fn solve_zero(&self) -> Result<Vec<f64>, SolveError> {
        self.solve(vec![0.0; self.n_cells])
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Precomputed face-to-cell connectivity and orthogonal distances for FV operators.
#[derive(Debug)]
pub struct FvMeshData {
    /// For each internal face: (owner, neighbour, distance, area_magnitude)
    pub internal_faces: Vec<(usize, usize, f64, f64)>,
    /// For each boundary face index: (cell_index, patch_index, distance, area_magnitude)
    pub boundary_faces: Vec<(usize, usize, f64, f64)>,
    /// Cell volumes
    pub cell_volumes: Vec<f64>,
    /// Face area vectors (outward from owner), indexed by global face index
    pub face_areas: Vec<Vector3>,
    /// Total number of faces
    pub n_faces: usize,
    /// Number of cells
    pub n_cells: usize,
}

impl FvMeshData {
    /// Compute all geometric data needed by FV operators from an orthogonal mesh.
    pub fn compute(mesh: &PolyMesh, geometry: &MeshGeometry) -> Self {
        let n_cells = geometry.cells.len();
        let n_faces = mesh.faces.len();
        let n_internal = mesh.neighbour.len();

        let mut internal_faces = Vec::with_capacity(n_internal);
        for face_i in 0..n_internal {
            let owner = mesh.owner[face_i];
            let neighbour = mesh.neighbour[face_i];
            let face_area_vec = geometry.faces[face_i].area;
            let area_mag = face_area_vec.magnitude();
            let normal = face_area_vec / area_mag;
            let delta = geometry.cells[neighbour].centre - geometry.cells[owner].centre;
            let distance = delta.dot(normal);
            debug_assert!(distance > 0.0, "non-orthogonal or inverted face {face_i}");
            internal_faces.push((owner, neighbour, distance, area_mag));
        }

        let face_patch_map = build_face_patch_map(mesh);
        let mut boundary_faces = Vec::with_capacity(n_faces - n_internal);
        for face_i in n_internal..n_faces {
            let cell = mesh.owner[face_i];
            let patch_idx = face_patch_map[face_i]
                .expect("boundary face must belong to a patch");
            let face_area_vec = geometry.faces[face_i].area;
            let area_mag = face_area_vec.magnitude();
            let normal = face_area_vec / area_mag;
            let delta = geometry.faces[face_i].centre - geometry.cells[cell].centre;
            let distance = delta.dot(normal);
            debug_assert!(distance > 0.0, "invalid boundary face distance at {face_i}");
            boundary_faces.push((cell, patch_idx, distance, area_mag));
        }

        let cell_volumes: Vec<f64> = geometry.cells.iter().map(|c| c.volume).collect();
        let face_areas: Vec<Vector3> = geometry.faces.iter().map(|f| f.area).collect();

        Self {
            internal_faces,
            boundary_faces,
            cell_volumes,
            face_areas,
            n_faces,
            n_cells,
        }
    }
}

fn build_face_patch_map(mesh: &PolyMesh) -> Vec<Option<usize>> {
    let mut map = vec![None; mesh.faces.len()];
    for (patch_i, patch) in mesh.patches.iter().enumerate() {
        for face_i in patch.start_face..patch.start_face + patch.face_count {
            map[face_i] = Some(patch_i);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fv_ops::assemble_laplacian;
    use crate::{MeshGeometry, PolyMesh};
    use std::path::Path;

    fn load_cavity_mesh() -> Option<(PolyMesh, MeshGeometry)> {
        let case_dir = Path::new("/mnt/secondary/openfoam/run/cavity");
        if !case_dir.join("constant/polyMesh/points").exists() {
            return None;
        }
        let mesh = PolyMesh::read_case(case_dir).ok()?;
        let geometry = mesh.geometry().ok()?;
        Some((mesh, geometry))
    }

    #[test]
    fn fv_matrix_simple_2cell() {
        // Two cells connected by one face: A = [[2, -1], [-1, 2]], b = [1, 2]
        // Solution: x0 = 4/3, x1 = 5/3
        let mut m = FvMatrix::new(2);
        m.add_diagonal(0, 2.0);
        m.add_diagonal(1, 2.0);
        m.add_off_diagonal(0, 1, -1.0);
        m.add_off_diagonal(1, 0, -1.0);
        m.source[0] = 1.0;
        m.source[1] = 2.0;
        let x = m.solve_zero().unwrap();
        assert!((x[0] - 4.0 / 3.0).abs() < 1e-10);
        assert!((x[1] - 5.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn fv_mesh_data_cavity() {
        let Some((mesh, geom)) = load_cavity_mesh() else { return };
        let data = FvMeshData::compute(&mesh, &geom);
        assert_eq!(data.n_cells, geom.cells.len());
        assert_eq!(data.internal_faces.len(), mesh.neighbour.len());
        for (_, _, dist, _) in &data.internal_faces {
            assert!(*dist > 0.0);
        }
        for (_, _, dist, _) in &data.boundary_faces {
            assert!(*dist > 0.0);
        }
    }

    #[test]
    fn laplacian_constant_field_gives_zero() {
        let Some((_mesh, _geom)) = load_cavity_mesh() else { return };
        let (mesh, geom) = load_cavity_mesh().unwrap();
        let data = FvMeshData::compute(&mesh, &geom);
        let n = data.n_cells;

        let mut mat = FvMatrix::new(n);
        assemble_laplacian(&mut mat, &data, 1.0, &vec![5.0; n]);

        // For a constant field with zeroGradient BCs everywhere, A*x should be ~0
        let x = vec![5.0; n];
        let mut ax = vec![0.0; n];
        for i in 0..n {
            ax[i] = mat.diagonal[i] * x[i]
                + mat.off_diagonal[i].iter().map(|&(j, a)| a * x[j]).sum::<f64>();
        }
        for i in 0..n {
            assert!(ax[i].abs() < 1e-10, "cell {i}: expected 0, got {}", ax[i]);
        }
    }
}
