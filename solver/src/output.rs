//! OpenFOAM ASCII field writers for solver output.

use crate::field::VolumeVectorField;
use crate::flow::Solution;
use crate::mesh::PolyMesh;
use std::fs;
use std::io;
use std::path::Path;

pub fn write_solution(
    mesh: &PolyMesh,
    input_u: &VolumeVectorField,
    solution: &Solution,
    dir: &Path,
    time: &str,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_vector(
        &dir.join("U"),
        time,
        &input_u.dimensions,
        mesh,
        input_u,
        &solution.velocity,
    )?;
    write_potential(&dir.join("Phi"), time, mesh, input_u, &solution.potential)?;
    write_flux(&dir.join("phi"), time, mesh, &solution.flux)?;
    Ok(())
}

fn write_vector(
    path: &Path,
    time: &str,
    dimensions: &str,
    mesh: &PolyMesh,
    input: &VolumeVectorField,
    values: &[crate::mesh::Vector3],
) -> io::Result<()> {
    let mut out = header("volVectorField", "U", time, dimensions);
    out.push_str(&format!(
        "internalField nonuniform List<vector>\n{}\n(\n",
        values.len()
    ));
    for value in values {
        out.push_str(&format!(
            "({:.12e} {:.12e} {:.12e})\n",
            value.x, value.y, value.z
        ));
    }
    out.push_str(")\nboundaryField\n{\n");
    for mesh_patch in &mesh.patches {
        let patch = input
            .boundary
            .iter()
            .find(|p| p.name == mesh_patch.name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing U patch {}", mesh_patch.name),
                )
            })?;
        let written_type = if matches!(patch.type_name.as_str(), "fixedValue" | "uniformFixedValue")
        {
            "fixedValue"
        } else {
            patch.type_name.as_str()
        };
        out.push_str(&format!(
            "    {}\n    {{\n        type {};\n",
            patch.name, written_type
        ));
        if matches!(patch.type_name.as_str(), "fixedValue" | "uniformFixedValue") {
            let value = patch.fixed_value.unwrap_or(crate::mesh::Vector3::zero());
            out.push_str(&format!(
                "        value uniform ({:.12e} {:.12e} {:.12e});\n",
                value.x, value.y, value.z
            ));
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    fs::write(path, out)
}

fn write_potential(
    path: &Path,
    time: &str,
    mesh: &PolyMesh,
    input: &VolumeVectorField,
    values: &[f64],
) -> io::Result<()> {
    let mut out = header("volScalarField", "Phi", time, "[0 2 -1 0 0 0 0]");
    out.push_str(&format!(
        "internalField nonuniform List<scalar>\n{}\n(\n",
        values.len()
    ));
    for value in values {
        out.push_str(&format!("{value:.12e}\n"));
    }
    out.push_str(")\nboundaryField\n{\n");
    for mesh_patch in &mesh.patches {
        let patch = input
            .boundary
            .iter()
            .find(|p| p.name == mesh_patch.name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing U patch {}", mesh_patch.name),
                )
            })?;
        let (kind, value) = match patch.type_name.as_str() {
            "zeroGradient" => ("fixedValue", Some(0.0)),
            "fixedValue" | "uniformFixedValue" => ("zeroGradient", None),
            other => (other, None),
        };
        out.push_str(&format!(
            "    {}\n    {{\n        type {};\n",
            mesh_patch.name, kind
        ));
        if let Some(value) = value {
            out.push_str(&format!("        value uniform {value:.12e};\n"));
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    fs::write(path, out)
}

fn write_flux(path: &Path, time: &str, mesh: &PolyMesh, flux: &[f64]) -> io::Result<()> {
    let mut out = header("surfaceScalarField", "phi", time, "[0 3 -1 0 0 0 0]");
    let n_internal = mesh.internal_face_count();
    out.push_str(&format!(
        "internalField nonuniform List<scalar>\n{}\n(\n",
        n_internal
    ));
    for value in &flux[..n_internal] {
        out.push_str(&format!("{value:.12e}\n"));
    }
    out.push_str(")\nboundaryField\n{\n");
    for patch in &mesh.patches {
        let end = patch.start_face + patch.face_count;
        out.push_str(&format!("    {}\n    {{\n        type calculated;\n        value nonuniform List<scalar>\n        {}\n        (\n", patch.name, patch.face_count));
        for value in &flux[patch.start_face..end] {
            out.push_str(&format!("            {value:.12e}\n"));
        }
        out.push_str("        );\n    }\n");
    }
    out.push_str("}\n");
    fs::write(path, out)
}

fn header(class: &str, object: &str, time: &str, dimensions: &str) -> String {
    format!("FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class {class};\n    location \"{time}\";\n    object {object};\n}}\n\ndimensions {dimensions};\n\n")
}
