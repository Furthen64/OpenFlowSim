//! Export OpenFOAM boundary faces to a Wavefront OBJ surface mesh.

use foamcore_solver::mesh::PolyMesh;
use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjExportSummary {
    pub vertex_count: usize,
    pub polygon_count: usize,
    pub patch_names: Vec<String>,
}

/// Write the selected mesh boundary patches to OBJ. An empty patch list exports
/// every boundary patch. Face winding follows OpenFOAM's owner-outward order.
pub fn write_boundary_obj(
    mesh: &PolyMesh,
    path: &Path,
    requested_patches: &[String],
) -> Result<ObjExportSummary, String> {
    let requested: HashSet<&str> = requested_patches.iter().map(String::as_str).collect();
    let missing: Vec<&str> = requested
        .iter()
        .copied()
        .filter(|name| !mesh.patches.iter().any(|patch| patch.name == *name))
        .collect();
    if !missing.is_empty() {
        return Err(format!("unknown mesh patch(es): {}", missing.join(", ")));
    }

    let selected: Vec<_> = mesh
        .patches
        .iter()
        .filter(|patch| requested.is_empty() || requested.contains(patch.name.as_str()))
        .collect();
    if selected.is_empty() {
        return Err("no boundary patches selected".into());
    }

    let mut used_points = vec![false; mesh.points.len()];
    let mut polygon_count = 0;
    for patch in &selected {
        let end = patch
            .start_face
            .checked_add(patch.face_count)
            .ok_or_else(|| format!("face range overflows for patch '{}'", patch.name))?;
        let faces = mesh
            .faces
            .get(patch.start_face..end)
            .ok_or_else(|| format!("invalid face range for patch '{}'", patch.name))?;
        for face in faces {
            if face.len() < 3 {
                return Err(format!("patch '{}' contains a degenerate face", patch.name));
            }
            for &point in face {
                let used = used_points.get_mut(point).ok_or_else(|| {
                    format!(
                        "face on patch '{}' references missing point {point}",
                        patch.name
                    )
                })?;
                *used = true;
            }
            polygon_count += 1;
        }
    }
    if polygon_count == 0 {
        return Err("selected patches contain no boundary faces".into());
    }

    let vertex_count = used_points.iter().filter(|&&used| used).count();
    let mut obj_indices = vec![0usize; mesh.points.len()];
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    writeln!(
        writer,
        "# Exterior boundary surface exported from OpenFOAM polyMesh"
    )
    .map_err(|e| e.to_string())?;
    writeln!(writer, "o openfoam_boundary").map_err(|e| e.to_string())?;
    let mut next_obj_index = 1usize;
    for (point_i, point) in mesh.points.iter().enumerate() {
        if !used_points[point_i] {
            continue;
        }
        obj_indices[point_i] = next_obj_index;
        next_obj_index += 1;
        writeln!(
            writer,
            "v {:.12e} {:.12e} {:.12e}",
            point.x, point.y, point.z
        )
        .map_err(|e| e.to_string())?;
    }
    for patch in selected {
        writeln!(writer, "\ng {}", obj_group_name(&patch.name)).map_err(|e| e.to_string())?;
        let end = patch.start_face + patch.face_count;
        for face in &mesh.faces[patch.start_face..end] {
            write!(writer, "f").map_err(|e| e.to_string())?;
            for &point_i in face {
                write!(writer, " {}", obj_indices[point_i]).map_err(|e| e.to_string())?;
            }
            writeln!(writer).map_err(|e| e.to_string())?;
        }
    }
    writer.flush().map_err(|e| e.to_string())?;

    Ok(ObjExportSummary {
        vertex_count,
        polygon_count,
        patch_names: mesh
            .patches
            .iter()
            .filter(|patch| requested.is_empty() || requested.contains(patch.name.as_str()))
            .map(|patch| patch.name.clone())
            .collect(),
    })
}

fn obj_group_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-.:".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use foamcore_solver::mesh::{BoundaryPatch, Vector3};

    fn sample_mesh() -> PolyMesh {
        PolyMesh {
            points: vec![
                Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                Vector3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                Vector3 {
                    x: 1.0,
                    y: 1.0,
                    z: 0.0,
                },
                Vector3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
                Vector3 {
                    x: 2.0,
                    y: 0.0,
                    z: 0.0,
                },
                Vector3 {
                    x: 2.0,
                    y: 1.0,
                    z: 0.0,
                },
                Vector3 {
                    x: 9.0,
                    y: 9.0,
                    z: 9.0,
                },
            ],
            faces: vec![
                vec![0, 1, 2], // internal face, excluded from boundary patches
                vec![0, 1, 3],
                vec![1, 4, 5, 2],
            ],
            owner: vec![0, 0, 1],
            neighbour: vec![1],
            patches: vec![
                BoundaryPatch {
                    name: "inlet".into(),
                    patch_type: "patch".into(),
                    start_face: 1,
                    face_count: 1,
                },
                BoundaryPatch {
                    name: "outlet wall".into(),
                    patch_type: "wall".into(),
                    start_face: 2,
                    face_count: 1,
                },
            ],
        }
    }

    #[test]
    fn exports_boundary_faces_and_patch_groups() {
        let path = std::env::temp_dir().join("foamcore-boundary-export.obj");
        let summary = write_boundary_obj(&sample_mesh(), &path, &[]).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(summary.vertex_count, 6);
        assert_eq!(summary.polygon_count, 2);
        assert!(contents.contains("g inlet"));
        assert!(contents.contains("g outlet_wall"));
        assert_eq!(
            contents
                .lines()
                .filter(|line| line.starts_with("f "))
                .count(),
            2
        );
        assert!(!contents.contains("v 9.000000000000e0"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn can_select_patches_and_reject_unknown_names() {
        let path = std::env::temp_dir().join("foamcore-boundary-export-selected.obj");
        let summary = write_boundary_obj(&sample_mesh(), &path, &["inlet".into()]).unwrap();
        assert_eq!(summary.patch_names, vec!["inlet"]);
        assert_eq!(summary.polygon_count, 1);
        assert!(write_boundary_obj(&sample_mesh(), &path, &["missing".into()]).is_err());
        let _ = std::fs::remove_file(path);
    }
}
