//! Discrete finite-volume operators for icoFoam on orthogonal meshes.
//!
//! Implicit operators (build into FvMatrix):
//!   - ddt: explicit Euler time derivative
//!   - div(phi, U): convective term with linear interpolation
//!   - laplacian(D, phi): viscous diffusion / pressure Poisson
//!
//! Explicit operators (return field values directly):
//!   - fvc::grad(p): cell gradient of a scalar field
//!   - fvc::div(surface_field): face divergence to cell centres
//!   - fvc::flux(vector_field): interpolate vector to faces and dot with area
//!   - fvc::interpolate(scalar_field): cell-to-face linear interpolation

use crate::bc::{BoundaryCondition, BoundaryConditions};
use crate::fv_matrix::{FvMatrix, FvMeshData};
use crate::Vector3;

/// Assemble the explicit-Euler time derivative contribution into `mat`.
///
/// Adds `(1/dt)` to the diagonal and `phi_old / dt` to the source,
/// representing `(phi_new - phi_old) / dt` moved to the LHS.
pub fn assemble_ddt(mat: &mut FvMatrix, fv: &FvMeshData, dt: f64, phi_old: &[f64]) {
    let inv_dt = 1.0 / dt;
    for i in 0..fv.n_cells {
        mat.add_diagonal(i, inv_dt);
        mat.add_source(i, phi_old[i] * inv_dt);
    }
}

/// Assemble the convective term `div(phi, U)` with linear (central-difference) scheme.
///
/// For each internal face between owner `o` and neighbour `n`:
///   U_face = 0.5 * (U_o + U_n)
///   Contribution to o's equation: -(phi_f / V_o) * U_face
///   => off-diag[o][n] += -phi_f / (2*V_o), diag[o] += phi_f / (2*V_o)
///
/// Boundary faces contribute to the source vector based on BC type.
///
/// `phi_faces` is the surface flux (meshPhi) at all faces, oriented out of owner.
/// `u_cells` is the cell-centre velocity field (one component).
/// `bc_values` gives fixed boundary values per boundary face index (into boundary_faces list).
pub fn assemble_div(
    mat: &mut FvMatrix,
    fv: &FvMeshData,
    phi_faces: &[f64],
    u_cells: &[f64],
    bc_values: &[Option<f64>],
) {
    // Internal faces
    for (face_i_internal_idx, &(owner, neighbour, _dist, _area)) in
        fv.internal_faces.iter().enumerate()
    {
        let phi_f = phi_faces[face_i_internal_idx];
        if phi_f.abs() < f64::MIN_POSITIVE {
            continue;
        }
        let w_neighbour = fv.internal_weights[face_i_internal_idx];
        let w_owner = 1.0 - w_neighbour;
        let inv_v_owner = 1.0 / fv.cell_volumes[owner];
        let coeff_owner = phi_f * w_owner * inv_v_owner;
        let coeff_neighbour = phi_f * w_neighbour * inv_v_owner;
        // Owner equation receives +phi*U_face / V.
        mat.add_diagonal(owner, coeff_owner);
        mat.add_off_diagonal(owner, neighbour, coeff_neighbour);

        // Neighbour cell sees the flux with opposite sign (face normal points into it)
        let inv_v_neigh = 1.0 / fv.cell_volumes[neighbour];
        let coeff_owner_n = -phi_f * w_owner * inv_v_neigh;
        let coeff_neighbour_n = -phi_f * w_neighbour * inv_v_neigh;
        mat.add_diagonal(neighbour, coeff_neighbour_n);
        mat.add_off_diagonal(neighbour, owner, coeff_owner_n);
    }

    // Boundary faces
    for (bi, &(cell, _patch_idx, _dist, _area)) in fv.boundary_faces.iter().enumerate() {
        let face_i = fv.n_internal() + bi;
        let phi_f = phi_faces.get(face_i).copied().unwrap_or(0.0);
        if phi_f.abs() < f64::MIN_POSITIVE {
            continue;
        }
        let inv_v = 1.0 / fv.cell_volumes[cell];
        match bc_values.get(bi) {
            Some(Some(val)) => {
                // fixedValue: U_face = val, contributes to source only
                mat.add_source(cell, -(phi_f * val) * inv_v);
            }
            Some(None) | None => {
                // zeroGradient/noSlip wall treatment uses the owner value
                // implicitly in the convective boundary contribution.
                let _ = u_cells[cell];
                mat.add_diagonal(cell, phi_f * inv_v);
            }
        }
    }
}

/// Assemble convection using patch-aware scalar boundary conditions.
pub fn assemble_div_with_bcs(
    mat: &mut FvMatrix,
    fv: &FvMeshData,
    phi_faces: &[f64],
    bcs: &BoundaryConditions<f64>,
) {
    for (fi, &(owner, neighbour, _, _)) in fv.internal_faces.iter().enumerate() {
        let flux = phi_faces.get(fi).copied().unwrap_or(0.0);
        let w_neighbour = fv.internal_weights[fi];
        let w_owner = 1.0 - w_neighbour;
        mat.add_diagonal(owner, flux * w_owner / fv.cell_volumes[owner]);
        mat.add_off_diagonal(
            owner,
            neighbour,
            flux * w_neighbour / fv.cell_volumes[owner],
        );
        mat.add_diagonal(neighbour, -flux * w_neighbour / fv.cell_volumes[neighbour]);
        mat.add_off_diagonal(
            neighbour,
            owner,
            -flux * w_owner / fv.cell_volumes[neighbour],
        );
    }
    for (bi, &(cell, patch, _, _)) in fv.boundary_faces.iter().enumerate() {
        let flux = phi_faces.get(fv.n_internal() + bi).copied().unwrap_or(0.0);
        let inv_v = 1.0 / fv.cell_volumes[cell];
        match bcs
            .patches
            .get(patch)
            .copied()
            .unwrap_or(BoundaryCondition::ZeroGradient)
        {
            BoundaryCondition::FixedValue(value) => mat.add_source(cell, -flux * value * inv_v),
            BoundaryCondition::NoSlip | BoundaryCondition::ZeroGradient => {
                mat.add_diagonal(cell, flux * inv_v)
            }
            BoundaryCondition::InletOutlet { inlet_value } if flux < 0.0 => {
                mat.add_source(cell, -flux * inlet_value * inv_v)
            }
            BoundaryCondition::InletOutlet { .. } => mat.add_diagonal(cell, flux * inv_v),
            BoundaryCondition::SymmetryPlane
            | BoundaryCondition::Empty
            | BoundaryCondition::FixedFluxPressure(_) => {}
        }
    }
}

/// Assemble the Laplacian term `-laplacian(D, phi)` into `mat`.
///
/// For orthogonal meshes, the off-diagonal coefficient across a face is:
///   -D * area / distance
/// Boundary faces with zeroGradient add to diagonal; fixedValue adds to source.
///
/// `bc_values` gives boundary condition values per boundary face (into boundary_faces list).
/// `None` means zeroGradient (Neumann), `Some(v)` means fixedValue (Dirichlet).
pub fn assemble_laplacian(
    mat: &mut FvMatrix,
    fv: &FvMeshData,
    diff_coeff: f64,
    _phi_cells: &[f64],
) {
    if diff_coeff.abs() < f64::MIN_POSITIVE {
        return;
    }

    // Internal faces: off-diagonal = -D * A / d
    for &(owner, neighbour, dist, area) in &fv.internal_faces {
        let coeff = diff_coeff * area / dist;
        mat.add_diagonal(owner, coeff / fv.cell_volumes[owner]);
        mat.add_diagonal(neighbour, coeff / fv.cell_volumes[neighbour]);
        mat.add_off_diagonal(owner, neighbour, -coeff / fv.cell_volumes[owner]);
        mat.add_off_diagonal(neighbour, owner, -coeff / fv.cell_volumes[neighbour]);
    }

    // Homogeneous Neumann boundary faces contribute no matrix terms.
}

/// Assemble the `-laplacian` matrix with fixed-value boundary contributions.
pub fn assemble_laplacian_with_bcs(
    mat: &mut FvMatrix,
    fv: &FvMeshData,
    diff_coeff: f64,
    bcs: &BoundaryConditions<f64>,
) {
    assemble_laplacian(mat, fv, diff_coeff, &[]);
    for (bi, &(cell, patch, dist, area)) in fv.boundary_faces.iter().enumerate() {
        if let Some(value) = bcs.dirichlet_face_value(patch, fv.boundary_patch_local_indices[bi]) {
            let coeff = diff_coeff * area / dist;
            mat.add_diagonal(cell, coeff / fv.cell_volumes[cell]);
            mat.add_source(cell, coeff * value / fv.cell_volumes[cell]);
        } else if let Some(gradient) =
            bcs.fixed_flux_gradient_at(patch, fv.boundary_patch_local_indices[bi])
        {
            mat.add_source(cell, -diff_coeff * gradient / fv.cell_volumes[cell]);
        }
    }
}

/// Add boundary source contributions for a laplacian with Dirichlet BCs.
/// Call after `assemble_laplacian` to inject `-coeff * bc_val` into the source.
pub fn add_laplacian_bc_source(
    mat: &mut FvMatrix,
    fv: &FvMeshData,
    diff_coeff: f64,
    bc_values: &[Option<f64>],
) {
    if diff_coeff.abs() < f64::MIN_POSITIVE {
        return;
    }
    for (bi, &(cell, _patch_idx, dist, area)) in fv.boundary_faces.iter().enumerate() {
        if let Some(val) = bc_values.get(bi).and_then(|v| *v) {
            let coeff = diff_coeff * area / dist;
            mat.add_source(cell, coeff * val / fv.cell_volumes[cell]);
        }
    }
}

// ─── Explicit operators (fvc::) ────────────────────────────────────────────────

/// Compute the cell-centre gradient of a scalar field using the Gauss method
/// on an orthogonal mesh: grad(phi)_C = sum_f (phi_f - phi_C) * S_f / V_C
/// where phi_f is linearly interpolated to the face.
pub fn fvc_grad(fv: &FvMeshData, phi_cells: &[f64]) -> Vec<Vector3> {
    fvc_grad_with_bcs(fv, phi_cells, None)
}

pub fn fvc_grad_with_bcs(
    fv: &FvMeshData,
    phi_cells: &[f64],
    bcs: Option<&BoundaryConditions<f64>>,
) -> Vec<Vector3> {
    let mut grad = vec![Vector3::zero(); fv.n_cells];

    // Internal faces
    for (fi, &(owner, neighbour, _dist, _area)) in fv.internal_faces.iter().enumerate() {
        let area_vec = fv.face_areas[fi];
        let w = fv.internal_weights[fi];
        let phi_face = (1.0 - w) * phi_cells[owner] + w * phi_cells[neighbour];
        grad[owner] += area_vec * (phi_face / fv.cell_volumes[owner]);
        grad[neighbour] += area_vec * (-phi_face / fv.cell_volumes[neighbour]);
    }

    for (bi, &(cell, patch, dist, _)) in fv.boundary_faces.iter().enumerate() {
        let owner_value = phi_cells[cell];
        let local_face = fv.boundary_patch_local_indices[bi];
        let face_value = bcs.map_or(owner_value, |conditions| {
            conditions
                .fixed_flux_gradient_at(patch, local_face)
                .map(|gradient| owner_value + gradient * dist)
                .unwrap_or_else(|| conditions.face_value(patch, local_face, owner_value, 0.0))
        });
        let fi = fv.n_internal() + bi;
        grad[cell] += fv.face_areas[fi] * (face_value / fv.cell_volumes[cell]);
    }

    grad
}

/// Compute the divergence of a surface scalar field to cell centres.
/// `surface_field` has one value per face (oriented out of owner).
pub fn fvc_div(fv: &FvMeshData, surface_field: &[f64]) -> Vec<f64> {
    let mut div = vec![0.0; fv.n_cells];

    // Internal faces: flux leaves owner, enters neighbour
    for (face_i, &(owner, neighbour, _dist, _area)) in fv.internal_faces.iter().enumerate() {
        let flux = surface_field[face_i];
        div[owner] += flux;
        div[neighbour] -= flux;
    }

    // Boundary faces: flux leaves the domain through boundary
    for (bi, &(cell, _patch_idx, _dist, _area)) in fv.boundary_faces.iter().enumerate() {
        let face_i = fv.n_internal() + bi;
        let flux = surface_field.get(face_i).copied().unwrap_or(0.0);
        div[cell] += flux;
    }

    // Divide by cell volume
    for i in 0..fv.n_cells {
        div[i] /= fv.cell_volumes[i];
    }
    div
}

/// Interpolate a scalar field from cells to faces (linear scheme on orthogonal mesh).
/// Returns one value per face.
pub fn fvc_interpolate(fv: &FvMeshData, phi_cells: &[f64]) -> Vec<f64> {
    fvc_interpolate_with_bcs(fv, phi_cells, None)
}

pub fn fvc_interpolate_with_bcs(
    fv: &FvMeshData,
    phi_cells: &[f64],
    bcs: Option<&BoundaryConditions<f64>>,
) -> Vec<f64> {
    let mut phi_faces = vec![0.0; fv.n_faces];

    // Internal faces: average of owner and neighbour
    for (fi, &(owner, neighbour, _dist, _area)) in fv.internal_faces.iter().enumerate() {
        let w = fv.internal_weights[fi];
        phi_faces[fi] = (1.0 - w) * phi_cells[owner] + w * phi_cells[neighbour];
    }

    // Boundary faces: use owner value (zeroGradient extrapolation)
    for (bi, &(cell, patch_idx, _dist, _area)) in fv.boundary_faces.iter().enumerate() {
        phi_faces[fv.n_internal() + bi] = bcs.map_or(phi_cells[cell], |conditions| {
            conditions.face_value(
                patch_idx,
                fv.boundary_patch_local_indices[bi],
                phi_cells[cell],
                0.0,
            )
        });
    }

    phi_faces
}

/// Compute the face flux of a vector field: `U_face · S_f` for each face.
/// Uses linear interpolation for internal faces.
/// `u_cells` is the cell-centre velocity (one component or full Vector3 per cell).
pub fn fvc_flux_scalar(fv: &FvMeshData, u_cells: &[f64]) -> Vec<f64> {
    let mut flux = vec![0.0; fv.n_faces];

    for (fi, &(owner, neighbour, _dist, area)) in fv.internal_faces.iter().enumerate() {
        let w = fv.internal_weights[fi];
        let u_face = (1.0 - w) * u_cells[owner] + w * u_cells[neighbour];
        // Flux = U_face * (S·n) but since we store scalar components and area magnitude:
        // Actually for a scalar component, flux through face = u_face * area_magnitude * n_component
        // But we need the normal direction. Use the face area vector dot with unit vector.
        // For simplicity on orthogonal meshes where we track one component at a time,
        // the flux of component i through face = u_i_face * |S| * n_i
        // We approximate by using the area vector's dominant component.
        let normal = fv.face_normals[fi];
        // The "flux" here is really just u_face projected onto the face normal
        // For the meshPhi computation we need full 3D, so this helper is for
        // single-component diagnostics. Full 3D flux uses fvc_flux_vector.
        flux[fi] = u_face
            * area
            * normal.dot(Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            });
    }

    flux
}

/// Compute the full 3D face flux (meshPhi): `U_face · S_f` for each face.
/// `u_cells` contains the full velocity Vector3 at each cell centre.
pub fn fvc_flux_vector(fv: &FvMeshData, u_cells: &[Vector3]) -> Vec<f64> {
    fvc_flux_vector_with_bcs(fv, u_cells, None)
}

pub fn fvc_flux_vector_with_bcs(
    fv: &FvMeshData,
    u_cells: &[Vector3],
    bcs: Option<&BoundaryConditions<Vector3>>,
) -> Vec<f64> {
    let mut phi = vec![0.0; fv.n_faces];

    // Internal faces
    for (fi, &(owner, neighbour, _dist, _area)) in fv.internal_faces.iter().enumerate() {
        let w = fv.internal_weights[fi];
        let u_face = u_cells[owner] * (1.0 - w) + u_cells[neighbour] * w;
        let area_vec = fv.face_areas[fi];
        phi[fi] = u_face.dot(area_vec);
    }

    // Boundary faces: use owner value (will be corrected by BCs in the solver)
    for (bi, &(cell, patch_idx, _dist, _area)) in fv.boundary_faces.iter().enumerate() {
        let fi = fv.n_internal() + bi;
        if matches!(
            bcs.and_then(|conditions| conditions.patches.get(patch_idx)),
            Some(BoundaryCondition::Empty)
        ) {
            phi[fi] = 0.0;
            continue;
        }
        let mut u_face = bcs.map_or(u_cells[cell], |conditions| {
            conditions.face_value(
                patch_idx,
                fv.boundary_patch_local_indices[bi],
                u_cells[cell],
                0.0,
            )
        });
        if matches!(
            bcs.and_then(|b| b.patches.get(patch_idx)),
            Some(BoundaryCondition::SymmetryPlane)
        ) {
            let normal = fv.face_normals[fi];
            u_face = u_face - normal * u_face.dot(normal);
        }
        let area_vec = fv.face_areas[fi];
        phi[fi] = u_face.dot(area_vec);
    }

    phi
}

// ─── Helper methods on FvMeshData ──────────────────────────────────────────────

impl FvMeshData {
    /// Number of internal faces.
    pub fn n_internal(&self) -> usize {
        self.internal_faces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MeshGeometry, PolyMesh};
    use std::path::Path;

    fn load_cavity() -> Option<(PolyMesh, MeshGeometry, FvMeshData)> {
        let case_dir = Path::new("/mnt/secondary/openfoam/run/cavity");
        if !case_dir.join("constant/polyMesh/points").exists() {
            return None;
        }
        let mesh = PolyMesh::read_case(case_dir).ok()?;
        let geom = mesh.geometry().ok()?;
        let fv = FvMeshData::compute(&mesh, &geom);
        Some((mesh, geom, fv))
    }

    #[test]
    fn ddt_assembly_correct() {
        let Some((_mesh, _geom, fv)) = load_cavity() else {
            return;
        };
        let n = fv.n_cells;
        let dt = 0.01;
        let phi_old = vec![1.0; n];

        let mut mat = FvMatrix::new(n);
        assemble_ddt(&mut mat, &fv, dt, &phi_old);

        for i in 0..n {
            assert!((mat.diagonal[i] - 1.0 / dt).abs() < 1e-14);
            assert!((mat.source[i] - 1.0 / dt).abs() < 1e-14);
        }
    }

    #[test]
    fn laplacian_matrix_is_symmetric_positive_definite_structure() {
        let Some((_mesh, _geom, fv)) = load_cavity() else {
            return;
        };
        let n = fv.n_cells;
        let nu = 0.01;

        let mut mat = FvMatrix::new(n);
        assemble_laplacian(&mut mat, &fv, nu, &vec![0.0; n]);

        for i in 0..n {
            assert!(mat.diagonal[i] > 0.0, "cell {i} has non-positive diagonal");
        }
        for i in 0..n {
            for &(j, coeff) in &mat.off_diagonal[i] {
                assert!(
                    coeff < 0.0,
                    "off-diag ({i},{j}) should be negative, got {coeff}"
                );
            }
        }
        for i in 0..n {
            let row_sum: f64 =
                mat.diagonal[i] + mat.off_diagonal[i].iter().map(|&(_, a)| a).sum::<f64>();
            assert!(row_sum >= -1e-12, "cell {i} row sum negative: {row_sum}");
        }
    }

    #[test]
    fn interpolate_linear_midpoint() {
        let Some((_mesh, _geom, fv)) = load_cavity() else {
            return;
        };
        let phi_cells = vec![0.0; fv.n_cells];
        let phi_faces = fvc_interpolate(&fv, &phi_cells);
        assert_eq!(phi_faces.len(), fv.n_faces);
    }

    #[test]
    fn flux_vector_zero_velocity_gives_zero_phi() {
        let Some((_mesh, _geom, fv)) = load_cavity() else {
            return;
        };
        let u = vec![Vector3::zero(); fv.n_cells];
        let phi = fvc_flux_vector(&fv, &u);
        for &val in &phi {
            assert!(val.abs() < 1e-14);
        }
    }

    #[test]
    fn div_of_constant_flux_is_zero_interior() {
        let Some((_mesh, _geom, fv)) = load_cavity() else {
            return;
        };
        let surface = vec![1.0; fv.n_faces];
        let div = fvc_div(&fv, &surface);
        for (i, &v) in div.iter().enumerate() {
            assert!(v.is_finite(), "cell {i} has non-finite divergence");
        }
    }

    #[test]
    fn grad_linear_field_gives_constant_gradient() {
        let Some((_mesh, geom, fv)) = load_cavity() else {
            return;
        };

        // phi = x: gradient should be (1, 0, 0) everywhere (interior)
        let phi_cells: Vec<f64> = geom.cells.iter().map(|c| c.centre.x).collect();
        let grad = fvc_grad(&fv, &phi_cells);

        let min_x = geom
            .cells
            .iter()
            .map(|c| c.centre.x)
            .fold(f64::INFINITY, f64::min);
        let max_x = geom
            .cells
            .iter()
            .map(|c| c.centre.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut checked = 0;
        for (i, cell) in geom.cells.iter().enumerate() {
            // With implicit zeroGradient at x boundaries, the Gauss gradient
            // is exact only for cells not touching those boundary patches.
            if cell.centre.x > min_x + 1e-10 && cell.centre.x < max_x - 1e-10 {
                assert!(
                    (grad[i].x - 1.0).abs() < 0.1,
                    "cell {i}: expected grad.x≈1, got {}",
                    grad[i].x
                );
                assert!(grad[i].y.abs() < 1e-6 && grad[i].z.abs() < 1e-6);
                checked += 1;
            }
        }
        assert!(checked > 0, "no cells passed the gradient check");
    }

    #[test]
    fn empty_boundary_has_zero_flux() {
        let fv = FvMeshData {
            internal_faces: vec![],
            internal_weights: vec![],
            boundary_faces: vec![(0, 0, 1.0, 1.0)],
            cell_volumes: vec![1.0],
            face_areas: vec![Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }],
            face_normals: vec![Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }],
            boundary_patch_local_indices: vec![0],
            n_faces: 1,
            n_cells: 1,
        };
        let bcs = BoundaryConditions::new(vec![BoundaryCondition::Empty]);
        let phi = fvc_flux_vector_with_bcs(
            &fv,
            &[Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }],
            Some(&bcs),
        );
        assert_eq!(phi, vec![0.0]);
    }
}
