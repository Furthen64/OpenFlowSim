//! Orthogonal finite-volume potential solve and velocity reconstruction.

use crate::field::{VectorPatch, VolumeVectorField};
use crate::mesh::{MeshGeometry, PolyMesh, Vector3};

#[derive(Debug, thiserror::Error)]
pub enum SolveError {
    #[error("input U field has {got} internal values; mesh has {expected} cells")]
    CellCount { expected: usize, got: usize },
    #[error("unsupported velocity boundary condition '{kind}' on patch '{patch}'")]
    UnsupportedBoundary { patch: String, kind: String },
    #[error("patch '{patch}' is missing from the U field")]
    MissingPatch { patch: String },
    #[error("potential solve needs at least one zeroGradient velocity patch to set Phi")]
    MissingOutlet,
    #[error(
        "mesh is not orthogonal at face {face}; this first solver supports orthogonal meshes only"
    )]
    NonOrthogonal { face: usize },
    #[error("linear solver failed to converge (residual {residual:.4e})")]
    NoConvergence { residual: f64 },
}

#[derive(Debug)]
pub struct Solution {
    pub velocity: Vec<Vector3>,
    pub potential: Vec<f64>,
    /// Face flux oriented out of each face's owner cell.
    pub flux: Vec<f64>,
}

pub fn solve(
    mesh: &PolyMesh,
    geometry: &MeshGeometry,
    field: &VolumeVectorField,
) -> Result<Solution, SolveError> {
    let n_cells = geometry.cells.len();
    let mut velocity = match field.internal.as_slice() {
        [value] => vec![*value; n_cells],
        values if values.len() == n_cells => values.to_vec(),
        values => {
            return Err(SolveError::CellCount {
                expected: n_cells,
                got: values.len(),
            })
        }
    };

    let patches: Vec<&VectorPatch> = mesh
        .patches
        .iter()
        .map(|mesh_patch| {
            field
                .boundary
                .iter()
                .find(|field_patch| field_patch.name == mesh_patch.name)
                .ok_or_else(|| SolveError::MissingPatch {
                    patch: mesh_patch.name.clone(),
                })
        })
        .collect::<Result<_, _>>()?;
    let face_patch = face_patch_map(mesh);
    let mut dirichlet_patches = vec![false; mesh.patches.len()];
    let mut has_dirichlet = false;
    for (patch_i, patch) in patches.iter().enumerate() {
        match patch.type_name.as_str() {
            "zeroGradient" => {
                dirichlet_patches[patch_i] = true;
                has_dirichlet = true;
            }
            "fixedValue" | "uniformFixedValue" | "symmetryPlane" | "symmetry" | "empty" => {}
            kind => {
                return Err(SolveError::UnsupportedBoundary {
                    patch: mesh.patches[patch_i].name.clone(),
                    kind: kind.into(),
                })
            }
        }
    }
    if !has_dirichlet {
        return Err(SolveError::MissingOutlet);
    }

    let mut diagonal = vec![0.0; n_cells];
    let mut off_diagonal = vec![Vec::<(usize, f64)>::new(); n_cells];
    let mut rhs = vec![0.0; n_cells];
    let mut face_coeff = vec![0.0; mesh.faces.len()];
    let mut boundary_flux = vec![0.0; mesh.faces.len()];
    let boundary_value = vec![0.0; mesh.faces.len()];

    for face_i in 0..mesh.faces.len() {
        let face_geo = geometry.faces[face_i];
        let area = face_geo.area.magnitude();
        let normal = face_geo.area / area;
        let owner = mesh.owner[face_i];
        if let Some(&neighbour) = mesh.neighbour.get(face_i) {
            let delta = geometry.cells[neighbour].centre - geometry.cells[owner].centre;
            let distance = delta.dot(normal);
            let transverse = (delta - normal * distance).magnitude();
            if distance <= 0.0 || transverse > distance.abs() * 1.0e-7 {
                return Err(SolveError::NonOrthogonal { face: face_i });
            }
            let coeff = area / distance;
            face_coeff[face_i] = coeff;
            diagonal[owner] += coeff;
            diagonal[neighbour] += coeff;
            off_diagonal[owner].push((neighbour, -coeff));
            off_diagonal[neighbour].push((owner, -coeff));
        } else {
            let patch_i = face_patch[face_i].expect("boundary face must belong to a patch");
            let patch = patches[patch_i];
            if dirichlet_patches[patch_i] {
                let delta = face_geo.centre - geometry.cells[owner].centre;
                let distance = delta.dot(normal);
                let transverse = (delta - normal * distance).magnitude();
                if distance <= 0.0 || transverse > distance.abs() * 1.0e-7 {
                    return Err(SolveError::NonOrthogonal { face: face_i });
                }
                let coeff = area / distance;
                face_coeff[face_i] = coeff;
                diagonal[owner] += coeff;
                rhs[owner] += coeff * boundary_value[face_i];
            } else if matches!(patch.type_name.as_str(), "fixedValue" | "uniformFixedValue") {
                let value = patch.fixed_value.unwrap_or(Vector3::zero());
                boundary_flux[face_i] = value.dot(face_geo.area);
                rhs[owner] += boundary_flux[face_i];
            }
        }
    }

    let mut potential = vec![0.0; n_cells];
    conjugate_gradient(&diagonal, &off_diagonal, &rhs, &mut potential)?;

    let mut flux = vec![0.0; mesh.faces.len()];
    for face_i in 0..mesh.faces.len() {
        let owner = mesh.owner[face_i];
        if let Some(&neighbour) = mesh.neighbour.get(face_i) {
            flux[face_i] = face_coeff[face_i] * (potential[neighbour] - potential[owner]);
        } else if face_coeff[face_i] > 0.0 {
            flux[face_i] = face_coeff[face_i] * (boundary_value[face_i] - potential[owner]);
        } else {
            flux[face_i] = boundary_flux[face_i];
        }
    }

    velocity = reconstruct_velocity(mesh, geometry, &flux, &velocity);
    Ok(Solution {
        velocity,
        potential,
        flux,
    })
}

fn face_patch_map(mesh: &PolyMesh) -> Vec<Option<usize>> {
    let mut map = vec![None; mesh.faces.len()];
    for (patch_i, patch) in mesh.patches.iter().enumerate() {
        for face_i in patch.start_face..patch.start_face + patch.face_count {
            map[face_i] = Some(patch_i);
        }
    }
    map
}

fn conjugate_gradient(
    diagonal: &[f64],
    off_diagonal: &[Vec<(usize, f64)>],
    rhs: &[f64],
    solution: &mut [f64],
) -> Result<(), SolveError> {
    let n = rhs.len();
    let apply = |x: &[f64], out: &mut [f64]| {
        for i in 0..n {
            out[i] =
                diagonal[i] * x[i] + off_diagonal[i].iter().map(|(j, a)| a * x[*j]).sum::<f64>();
        }
    };
    let mut residual = vec![0.0; n];
    let mut product = vec![0.0; n];
    apply(solution, &mut product);
    for i in 0..n {
        residual[i] = rhs[i] - product[i];
    }
    let initial_norm = dot(&residual, &residual).sqrt();
    let rhs_norm = dot(rhs, rhs).sqrt().max(1.0);
    if initial_norm <= 1.0e-11 * rhs_norm {
        return Ok(());
    }
    let mut z: Vec<f64> = residual
        .iter()
        .enumerate()
        .map(|(i, r)| {
            if diagonal[i] > 0.0 {
                r / diagonal[i]
            } else {
                *r
            }
        })
        .collect();
    let mut direction = z.clone();
    let mut rz = dot(&residual, &z);
    for _ in 0..(n.saturating_mul(20).max(100)) {
        apply(&direction, &mut product);
        let denom = dot(&direction, &product);
        if denom <= f64::MIN_POSITIVE {
            break;
        }
        let alpha = rz / denom;
        for i in 0..n {
            solution[i] += alpha * direction[i];
            residual[i] -= alpha * product[i];
        }
        if dot(&residual, &residual).sqrt() <= 1.0e-11 * rhs_norm {
            return Ok(());
        }
        for i in 0..n {
            z[i] = if diagonal[i] > 0.0 {
                residual[i] / diagonal[i]
            } else {
                residual[i]
            };
        }
        let next_rz = dot(&residual, &z);
        let beta = next_rz / rz;
        for i in 0..n {
            direction[i] = z[i] + beta * direction[i];
        }
        rz = next_rz;
    }
    Err(SolveError::NoConvergence {
        residual: dot(&residual, &residual).sqrt(),
    })
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn reconstruct_velocity(
    mesh: &PolyMesh,
    geometry: &MeshGeometry,
    flux: &[f64],
    initial: &[Vector3],
) -> Vec<Vector3> {
    let mut matrices = vec![[[0.0; 3]; 3]; geometry.cells.len()];
    let mut vectors = vec![[0.0; 3]; geometry.cells.len()];
    for face_i in 0..mesh.faces.len() {
        let area_owner = geometry.faces[face_i].area;
        accumulate_normal_equation(
            &mut matrices,
            &mut vectors,
            mesh.owner[face_i],
            area_owner,
            flux[face_i],
        );
        if let Some(&neighbour) = mesh.neighbour.get(face_i) {
            accumulate_normal_equation(
                &mut matrices,
                &mut vectors,
                neighbour,
                area_owner * -1.0,
                -flux[face_i],
            );
        }
    }
    matrices
        .into_iter()
        .zip(vectors)
        .zip(initial)
        .map(|((a, b), fallback)| solve_symmetric_3(a, b).unwrap_or(*fallback))
        .collect()
}

fn accumulate_normal_equation(
    matrices: &mut [[[f64; 3]; 3]],
    vectors: &mut [[f64; 3]],
    cell: usize,
    area: Vector3,
    flux: f64,
) {
    let s = [area.x, area.y, area.z];
    for i in 0..3 {
        vectors[cell][i] += s[i] * flux;
        for j in 0..3 {
            matrices[cell][i][j] += s[i] * s[j];
        }
    }
}

fn solve_symmetric_3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<Vector3> {
    let scale = a.iter().flatten().copied().fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return None;
    }
    for column in 0..3 {
        let pivot =
            (column..3).max_by(|&i, &j| a[i][column].abs().total_cmp(&a[j][column].abs()))?;
        if a[pivot][column].abs() <= scale * 1.0e-12 {
            continue;
        }
        a.swap(column, pivot);
        b.swap(column, pivot);
        let divisor = a[column][column];
        for j in column..3 {
            a[column][j] /= divisor;
        }
        b[column] /= divisor;
        for i in 0..3 {
            if i == column {
                continue;
            }
            let factor = a[i][column];
            for j in column..3 {
                a[i][j] -= factor * a[column][j];
            }
            b[i] -= factor * b[column];
        }
    }
    Some(Vector3 {
        x: b[0],
        y: b[1],
        z: b[2],
    })
}
