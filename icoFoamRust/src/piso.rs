//! Orthogonal-mesh PISO time integration for incompressible laminar flow.

use crate::bc::{BoundaryCondition, BoundaryConditions};
use crate::fv_matrix::{FvMatrix, FvMeshData};
use crate::fv_ops::{
    assemble_div_with_bcs, assemble_laplacian_with_bcs, fvc_div, fvc_flux_vector_with_bcs,
    fvc_grad_with_bcs,
};
use crate::scalar_field::{BoundaryPatch, ScalarField};
use crate::{CaseSetup, PolyMesh, Vector3};
use foamcore_solver::field::VolumeVectorField;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub fn run_case(case_dir: &Path) -> Result<(), Box<dyn Error>> {
    let setup = CaseSetup::read(case_dir)?;
    let mesh = PolyMesh::read_case(case_dir)?;
    let geometry = mesh.geometry()?;
    let fv = FvMeshData::compute(&mesh, &geometry);
    let zero = case_dir.join("0");
    let u_path = zero.join("U");
    let p_path = zero.join("p");
    let input_u = VolumeVectorField::read(&u_path)?;
    let input_p = ScalarField::read(&p_path)?;
    let n = fv.n_cells;
    let mut u = expand_vectors(&input_u.internal, n);
    let mut p = expand_scalars(&input_p.internal, n);
    let u_bcs = vector_bcs_for_mesh(&input_u, &mesh);
    let mut p_bcs = scalar_bcs_for_mesh(&input_p, &mesh)?;
    let nu = setup.properties.nu;
    let mut phi = fvc_flux_vector_with_bcs(&fv, &u, Some(&u_bcs));
    let mut time = setup.control.start_time;
    let mut step = 0usize;
    while time < setup.control.end_time - 1.0e-12 {
        let dt = setup.control.delta_t.min(setup.control.end_time - time);
        let u_old = u.clone();
        let phi_old = phi.clone();
        let pressure_gradient = fvc_grad_with_bcs(&fv, &p, Some(&p_bcs));
        let mut momentum = FvMatrix::new(n);
        let mut h_source = [vec![0.0; n], vec![0.0; n], vec![0.0; n]];
        for i in 0..n {
            h_source[0][i] = u_old[i].x / dt;
            h_source[1][i] = u_old[i].y / dt;
            h_source[2][i] = u_old[i].z / dt;
        }
        let mut diagonal = vec![1.0 / dt; n];
        let mut assembled_source = [vec![0.0; n], vec![0.0; n], vec![0.0; n]];
        for comp in 0..3 {
            let mut eqn = FvMatrix::new(n);
            for i in 0..n {
                eqn.add_diagonal(i, 1.0 / dt);
                eqn.add_source(i, h_source[comp][i]);
            }
            let component_conditions = component_bcs(&u_bcs, comp);
            assemble_div_with_bcs(&mut eqn, &fv, &phi, &component_conditions);
            assemble_laplacian_with_bcs(&mut eqn, &fv, nu, &component_conditions);
            assembled_source[comp].clone_from(&eqn.source);
            if comp == 0 {
                momentum = eqn.clone();
                diagonal.clone_from(&eqn.diagonal);
            }
            for i in 0..n {
                eqn.add_source(i, -component(pressure_gradient[i], comp));
            }
            if setup.solution.piso.momentum_predictor {
                let initial: Vec<f64> = u.iter().map(|v| component(*v, comp)).collect();
                let solved = eqn.solve_bicgstab(initial)?;
                for (i, value) in solved.into_iter().enumerate() {
                    set_component(&mut u[i], comp, value);
                }
            }
        }
        let r_au: Vec<f64> = diagonal.iter().map(|d| 1.0 / d).collect();
        for _corrector in 0..setup.solution.piso.n_correctors.max(1) {
            let h_by_a: Vec<Vector3> = (0..n)
                .map(|i| {
                    let mut h = Vector3::zero();
                    for comp in 0..3 {
                        let offdiag: f64 = momentum.off_diagonal[i]
                            .iter()
                            .map(|&(j, a)| a * component(u[j], comp))
                            .sum();
                        set_component(
                            &mut h,
                            comp,
                            r_au[i] * (assembled_source[comp][i] - offdiag),
                        );
                    }
                    h
                })
                .collect();
            let mut phi_h_by_a = fvc_flux_vector_with_bcs(&fv, &h_by_a, Some(&u_bcs));
            constrain_fixed_flux_pressure(&fv, &u, &u_bcs, &mut p_bcs, &r_au, &mut phi_h_by_a);
            let old_flux_from_u = fvc_flux_vector_with_bcs(&fv, &u_old, Some(&u_bcs));
            for fi in 0..fv.n_faces {
                if fi >= fv.n_internal() {
                    continue;
                }
                let corr = phi_old[fi] - old_flux_from_u[fi];
                let coupling = if phi_old[fi].abs() > f64::MIN_POSITIVE {
                    (1.0 - corr.abs() / phi_old[fi].abs()).max(0.0)
                } else if corr.abs() < f64::MIN_POSITIVE {
                    1.0
                } else {
                    0.0
                };
                let rauf = if fi < fv.n_internal() {
                    let (owner, neighbour, _, _) = fv.internal_faces[fi];
                    let w = fv.internal_weights[fi];
                    (1.0 - w) * r_au[owner] + w * r_au[neighbour]
                } else {
                    let (cell, _, _, _) = fv.boundary_faces[fi - fv.n_internal()];
                    r_au[cell]
                };
                phi_h_by_a[fi] += coupling * corr / dt * rauf;
            }
            let mut phi_h = phi_h_by_a.clone();
            let mut p_eqn = FvMatrix::new(n);
            for (fi, &(owner, neighbour, dist, area)) in fv.internal_faces.iter().enumerate() {
                let w = fv.internal_weights[fi];
                let r_au_face = (1.0 - w) * r_au[owner] + w * r_au[neighbour];
                let coeff = r_au_face * area / dist;
                p_eqn.add_diagonal(owner, coeff / fv.cell_volumes[owner]);
                p_eqn.add_off_diagonal(owner, neighbour, -coeff / fv.cell_volumes[owner]);
                p_eqn.add_diagonal(neighbour, coeff / fv.cell_volumes[neighbour]);
                p_eqn.add_off_diagonal(neighbour, owner, -coeff / fv.cell_volumes[neighbour]);
                let _ = fi;
            }
            for (bi, &(cell, patch, dist, area)) in fv.boundary_faces.iter().enumerate() {
                if let Some(value) =
                    p_bcs.dirichlet_face_value(patch, fv.boundary_patch_local_indices[bi])
                {
                    let coeff = r_au[cell] * area / dist / fv.cell_volumes[cell];
                    p_eqn.add_diagonal(cell, coeff);
                    p_eqn.add_source(cell, coeff * value);
                } else if let Some(gradient) =
                    p_bcs.fixed_flux_gradient_at(patch, fv.boundary_patch_local_indices[bi])
                {
                    p_eqn.add_source(cell, -r_au[cell] * area * gradient / fv.cell_volumes[cell]);
                }
                let _ = bi;
            }
            let div_phi = fvc_div(&fv, &phi_h_by_a);
            for i in 0..n {
                p_eqn.add_source(i, -div_phi[i]);
            }
            let ref_cell = setup.solution.piso.p_ref_cell.min(n.saturating_sub(1));
            let has_pressure_reference = fv
                .boundary_faces
                .iter()
                .any(|(_, patch, _, _)| p_bcs.is_dirichlet(*patch).is_some());
            if !has_pressure_reference {
                p_eqn.set_reference(ref_cell, setup.solution.piso.p_ref_value);
            }
            p = p_eqn.solve_bicgstab(p.clone())?;
            for (fi, &(owner, neighbour, dist, area)) in fv.internal_faces.iter().enumerate() {
                let w = fv.internal_weights[fi];
                let r_au_face = (1.0 - w) * r_au[owner] + w * r_au[neighbour];
                let coeff = r_au_face * area / dist;
                let p_flux = coeff * (p[neighbour] - p[owner]);
                phi_h[fi] -= p_flux;
            }
            for (bi, &(cell, patch, dist, area)) in fv.boundary_faces.iter().enumerate() {
                let p_flux = if let Some(value) =
                    p_bcs.dirichlet_face_value(patch, fv.boundary_patch_local_indices[bi])
                {
                    r_au[cell] * area / dist * (value - p[cell])
                } else {
                    r_au[cell]
                        * area
                        * p_bcs
                            .fixed_flux_gradient_at(patch, fv.boundary_patch_local_indices[bi])
                            .unwrap_or(0.0)
                };
                phi_h[fv.n_internal() + bi] -= p_flux;
            }
            phi.clone_from(&phi_h);
            let grad_p = fvc_grad_with_bcs(&fv, &p, Some(&p_bcs));
            for i in 0..n {
                u[i] = h_by_a[i] - grad_p[i] * r_au[i];
            }
        }
        time += dt;
        step += 1;
        let mut face_sum = vec![0.0; n];
        for (fi, &(owner, neighbour, _, _)) in fv.internal_faces.iter().enumerate() {
            face_sum[owner] += phi[fi].abs();
            face_sum[neighbour] += phi[fi].abs();
        }
        for (bi, &(cell, _, _, _)) in fv.boundary_faces.iter().enumerate() {
            face_sum[cell] += phi[fv.n_internal() + bi].abs();
        }
        let max_courant = face_sum
            .iter()
            .zip(&fv.cell_volumes)
            .map(|(sum, v)| 0.5 * sum * dt / v)
            .fold(0.0_f64, f64::max);
        let divergence = fvc_div(&fv, &phi);
        let continuity_local: f64 = divergence
            .iter()
            .zip(&fv.cell_volumes)
            .map(|(d, volume)| d.abs() * volume)
            .sum();
        let continuity_global: f64 = divergence
            .iter()
            .zip(&fv.cell_volumes)
            .map(|(d, volume)| d * volume)
            .sum();
        println!(
            "Time = {}  Courant = {:.4e}  continuityLocal = {:.4e}  continuityGlobal = {:.4e}",
            time, max_courant, continuity_local, continuity_global
        );
        if setup.control.should_write(step) {
            write_fields(case_dir, time, &mesh, &input_u, &input_p, &u, &p, &phi)?;
        }
    }
    Ok(())
}

/// Set fixedFluxPressure gradients so the pressure correction preserves the
/// normal flux imposed by the velocity boundary condition, matching
/// OpenFOAM's constrainPressure step.
fn constrain_fixed_flux_pressure(
    fv: &FvMeshData,
    u: &[Vector3],
    u_bcs: &BoundaryConditions<Vector3>,
    p_bcs: &mut BoundaryConditions<f64>,
    r_au: &[f64],
    phi_h_by_a: &mut [f64],
) {
    let velocity_flux = fvc_flux_vector_with_bcs(fv, u, Some(u_bcs));
    for (bi, &(cell, patch, _, area)) in fv.boundary_faces.iter().enumerate() {
        if !matches!(
            p_bcs.patches.get(patch),
            Some(BoundaryCondition::FixedFluxPressure(_))
        ) {
            continue;
        }
        let face = fv.n_internal() + bi;
        let denominator = r_au[cell] * area;
        if denominator.abs() > f64::MIN_POSITIVE {
            let gradient = (phi_h_by_a[face] - velocity_flux[face]) / denominator;
            p_bcs.set_fixed_flux_gradient_at(patch, fv.boundary_patch_local_indices[bi], gradient);
        }
    }
}

fn expand_vectors(values: &[Vector3], n: usize) -> Vec<Vector3> {
    if values.len() == 1 {
        vec![values[0]; n]
    } else {
        values.to_vec()
    }
}
fn expand_scalars(values: &[f64], n: usize) -> Vec<f64> {
    if values.len() == 1 {
        vec![values[0]; n]
    } else {
        values.to_vec()
    }
}
fn component(v: Vector3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}
fn set_component(v: &mut Vector3, i: usize, x: f64) {
    match i {
        0 => v.x = x,
        1 => v.y = x,
        _ => v.z = x,
    }
}

fn vector_bcs_for_mesh(field: &VolumeVectorField, mesh: &PolyMesh) -> BoundaryConditions<Vector3> {
    let patches = mesh
        .patches
        .iter()
        .map(|mp| field.boundary.iter().find(|p| p.name == mp.name))
        .map(|patch| {
            match patch
                .map(|p| p.type_name.as_str())
                .unwrap_or("zeroGradient")
            {
                "fixedValue" | "uniformFixedValue" => BoundaryCondition::FixedValue(
                    patch.and_then(|p| p.fixed_value).unwrap_or(Vector3::zero()),
                ),
                "noSlip" => BoundaryCondition::NoSlip,
                "empty" => BoundaryCondition::Empty,
                "symmetry" | "symmetryPlane" => BoundaryCondition::SymmetryPlane,
                _ => BoundaryCondition::ZeroGradient,
            }
        })
        .collect();
    BoundaryConditions::<Vector3>::new(patches)
}

fn component_bcs(bcs: &BoundaryConditions<Vector3>, component_i: usize) -> BoundaryConditions<f64> {
    let patches = bcs
        .patches
        .iter()
        .map(|bc| match *bc {
            BoundaryCondition::FixedValue(value) => {
                BoundaryCondition::FixedValue(component(value, component_i))
            }
            BoundaryCondition::NoSlip => BoundaryCondition::NoSlip,
            BoundaryCondition::SymmetryPlane => BoundaryCondition::SymmetryPlane,
            BoundaryCondition::Empty => BoundaryCondition::Empty,
            BoundaryCondition::InletOutlet { inlet_value } => BoundaryCondition::InletOutlet {
                inlet_value: component(inlet_value, component_i),
            },
            BoundaryCondition::FixedFluxPressure(g) => BoundaryCondition::FixedFluxPressure(g),
            BoundaryCondition::ZeroGradient => BoundaryCondition::ZeroGradient,
        })
        .collect();
    BoundaryConditions::<f64>::new(patches)
}

fn scalar_bcs_for_mesh(
    field: &ScalarField,
    mesh: &PolyMesh,
) -> Result<BoundaryConditions<f64>, Box<dyn Error>> {
    let ordered: Vec<BoundaryPatch> = mesh
        .patches
        .iter()
        .map(|mp| {
            field
                .boundary
                .iter()
                .find(|p| p.name == mp.name)
                .cloned()
                .unwrap_or(BoundaryPatch {
                    name: mp.name.clone(),
                    bc_type: "zeroGradient".into(),
                    values: None,
                })
        })
        .collect();
    Ok(BoundaryConditions::from_scalar_patches(&ordered)?)
}

fn write_fields(
    case_dir: &Path,
    time: f64,
    mesh: &PolyMesh,
    input_u: &VolumeVectorField,
    input_p: &ScalarField,
    u: &[Vector3],
    p: &[f64],
    phi: &[f64],
) -> Result<(), Box<dyn Error>> {
    let time_name = crate::time_dirs::format_time(time);
    let dir: PathBuf = case_dir.join(&time_name);
    fs::create_dir_all(&dir)?;
    let p_field = ScalarField {
        dimensions: input_p.dimensions.clone(),
        internal: p.to_vec(),
        boundary: input_p.boundary.clone(),
    };
    p_field.write(&dir.join("p"), "volScalarField", &time_name, "p")?;
    let mut u_text = format!("FoamFile\n{{ version 2.0; format ascii; class volVectorField; location \"{time_name}\"; object U; }}\n\ndimensions {};\ninternalField nonuniform List<vector>\n{}\n(\n", input_u.dimensions, u.len());
    for value in u {
        u_text.push_str(&format!(
            "({:.12e} {:.12e} {:.12e})\n",
            value.x, value.y, value.z
        ));
    }
    u_text.push_str(")\nboundaryField\n{\n");
    for patch in &input_u.boundary {
        u_text.push_str(&format!("{} {{ type {};", patch.name, patch.type_name));
        if let Some(value) = patch.fixed_value {
            u_text.push_str(&format!(
                " value uniform ({:.12e} {:.12e} {:.12e});",
                value.x, value.y, value.z
            ));
        }
        u_text.push_str(" }\n");
    }
    u_text.push_str("}\n");
    fs::write(dir.join("U"), u_text)?;
    let internal = phi[..mesh.internal_face_count()].to_vec();
    let boundary = mesh
        .patches
        .iter()
        .map(|patch| BoundaryPatch {
            name: patch.name.clone(),
            bc_type: "calculated".into(),
            values: Some(phi[patch.start_face..patch.start_face + patch.face_count].to_vec()),
        })
        .collect();
    let phi_field = ScalarField {
        dimensions: "[0 3 -1 0 0 0 0]".into(),
        internal,
        boundary,
    };
    phi_field.write(&dir.join("phi"), "surfaceScalarField", &time_name, "phi")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_flux_pressure_matches_velocity_boundary_flux() {
        let fv = FvMeshData {
            internal_faces: vec![],
            internal_weights: vec![],
            boundary_faces: vec![(0, 0, 1.0, 2.0)],
            cell_volumes: vec![1.0],
            face_areas: vec![Vector3 {
                x: 2.0,
                y: 0.0,
                z: 0.0,
            }],
            face_normals: vec![Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            }],
            boundary_patch_local_indices: vec![0],
            n_faces: 1,
            n_cells: 1,
        };
        let u_bcs = BoundaryConditions::new(vec![BoundaryCondition::FixedValue(Vector3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        })]);
        let mut p_bcs = BoundaryConditions::new(vec![BoundaryCondition::FixedFluxPressure(0.0)]);
        let mut phi_h_by_a = vec![5.0];
        constrain_fixed_flux_pressure(
            &fv,
            &[Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            }],
            &u_bcs,
            &mut p_bcs,
            &[2.0],
            &mut phi_h_by_a,
        );
        let gradient = p_bcs.fixed_flux_gradient_at(0, 0).unwrap();
        let corrected_flux = phi_h_by_a[0] - 2.0 * 2.0 * gradient;
        assert!((corrected_flux - 2.0).abs() < 1.0e-12);
    }
}
