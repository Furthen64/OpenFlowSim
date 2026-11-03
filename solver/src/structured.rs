//! Small structured-box mesh generator used when a corpus case has no
//! pre-generated `constant/polyMesh` (initially, `simpleChannel`).

use crate::mesh::{BoundaryPatch, PolyMesh, Vector3};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StructuredMeshError {
    #[error("failed to read case metadata: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid structured case metadata: {0}")]
    Invalid(String),
}

pub fn from_case(case_dir: &Path) -> Result<PolyMesh, StructuredMeshError> {
    let meta: toml::Value = toml::from_str(&std::fs::read_to_string(case_dir.join("case.toml"))?)
        .map_err(|e| StructuredMeshError::Invalid(e.to_string()))?;
    let geometry = meta
        .get("geometry")
        .ok_or_else(|| StructuredMeshError::Invalid("missing [geometry]".into()))?;
    let bounds = |key: &str| -> Result<[f64; 3], StructuredMeshError> {
        let array = geometry
            .get(key)
            .and_then(toml::Value::as_array)
            .ok_or_else(|| StructuredMeshError::Invalid(format!("missing geometry.{key}")))?;
        if array.len() != 3 {
            return Err(StructuredMeshError::Invalid(format!(
                "geometry.{key} must have 3 values"
            )));
        }
        Ok([
            array[0]
                .as_float()
                .ok_or_else(|| StructuredMeshError::Invalid(format!("invalid geometry.{key}")))?,
            array[1]
                .as_float()
                .ok_or_else(|| StructuredMeshError::Invalid(format!("invalid geometry.{key}")))?,
            array[2]
                .as_float()
                .ok_or_else(|| StructuredMeshError::Invalid(format!("invalid geometry.{key}")))?,
        ])
    };
    let minimum = bounds("domain_min")?;
    let maximum = bounds("domain_max")?;
    let cells = geometry
        .get("cells_per_axis")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| StructuredMeshError::Invalid("missing geometry.cells_per_axis".into()))?;
    if cells.len() != 3 {
        return Err(StructuredMeshError::Invalid(
            "cells_per_axis must have 3 values".into(),
        ));
    }
    let nx = cells[0].as_integer().unwrap_or(0) as usize;
    let ny = cells[1].as_integer().unwrap_or(0) as usize;
    let nz = cells[2].as_integer().unwrap_or(0) as usize;
    if nx == 0 || ny == 0 || nz != 1 || (0..3).any(|i| maximum[i] <= minimum[i]) {
        return Err(StructuredMeshError::Invalid(
            "only positive, one-cell-thick structured 2D boxes are currently supported".into(),
        ));
    }
    let dx = (maximum[0] - minimum[0]) / nx as f64;
    let dy = (maximum[1] - minimum[1]) / ny as f64;
    let dz = maximum[2] - minimum[2];
    let point_id = |i: usize, j: usize, k: usize| i + (nx + 1) * (j + (ny + 1) * k);
    let cell_id = |i: usize, j: usize| i + nx * j;

    let mut points = Vec::with_capacity((nx + 1) * (ny + 1) * 2);
    for k in 0..=1 {
        for j in 0..=ny {
            for i in 0..=nx {
                points.push(Vector3 {
                    x: minimum[0] + i as f64 * dx,
                    y: minimum[1] + j as f64 * dy,
                    z: minimum[2] + k as f64 * dz,
                });
            }
        }
    }
    let quad_x = |i: usize, j: usize| -> Vec<usize> {
        vec![
            point_id(i, j, 0),
            point_id(i, j + 1, 0),
            point_id(i, j + 1, 1),
            point_id(i, j, 1),
        ]
    };
    let quad_y = |i: usize, j: usize| -> Vec<usize> {
        vec![
            point_id(i, j, 0),
            point_id(i, j, 1),
            point_id(i + 1, j, 1),
            point_id(i + 1, j, 0),
        ]
    };
    let quad_z = |i: usize, j: usize, k: usize| -> Vec<usize> {
        vec![
            point_id(i, j, k),
            point_id(i + 1, j, k),
            point_id(i + 1, j + 1, k),
            point_id(i, j + 1, k),
        ]
    };
    let mut faces = Vec::new();
    let mut owner = Vec::new();
    let mut neighbour = Vec::new();
    for i in 1..nx {
        for j in 0..ny {
            faces.push(quad_x(i, j));
            owner.push(cell_id(i - 1, j));
            neighbour.push(cell_id(i, j));
        }
    }
    for j in 1..ny {
        for i in 0..nx {
            faces.push(quad_y(i, j));
            owner.push(cell_id(i, j - 1));
            neighbour.push(cell_id(i, j));
        }
    }
    let n_internal = faces.len();

    let boundary_conditions = meta
        .get("boundary_conditions")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| StructuredMeshError::Invalid("missing [boundary_conditions]".into()))?;
    let patch_specs = [
        ("inlet", "patch", 0),
        ("outlet", "patch", 1),
        ("wall_bottom", "symmetryPlane", 2),
        ("wall_top", "symmetryPlane", 3),
        ("frontAndBack", "empty", 4),
    ];
    let mut patches = Vec::new();
    for (name, patch_type, side) in patch_specs {
        if !boundary_conditions.contains_key(name) {
            continue;
        }
        let start_face = faces.len();
        match side {
            0 => {
                for j in 0..ny {
                    faces.push({
                        let mut f = quad_x(0, j);
                        f.reverse();
                        f
                    });
                    owner.push(cell_id(0, j));
                }
            }
            1 => {
                for j in 0..ny {
                    faces.push(quad_x(nx, j));
                    owner.push(cell_id(nx - 1, j));
                }
            }
            2 => {
                for i in 0..nx {
                    faces.push({
                        let mut f = quad_y(i, 0);
                        f.reverse();
                        f
                    });
                    owner.push(cell_id(i, 0));
                }
            }
            3 => {
                for i in 0..nx {
                    faces.push(quad_y(i, ny));
                    owner.push(cell_id(i, ny - 1));
                }
            }
            4 => {
                for j in 0..ny {
                    for i in 0..nx {
                        faces.push({
                            let mut f = quad_z(i, j, 0);
                            f.reverse();
                            f
                        });
                        owner.push(cell_id(i, j));
                    }
                }
                for j in 0..ny {
                    for i in 0..nx {
                        faces.push(quad_z(i, j, 1));
                        owner.push(cell_id(i, j));
                    }
                }
            }
            _ => unreachable!(),
        }
        patches.push(BoundaryPatch {
            name: name.into(),
            patch_type: patch_type.into(),
            start_face,
            face_count: faces.len() - start_face,
        });
    }
    if faces.len() - n_internal != 2 * nx + 2 * ny + 2 * nx * ny {
        return Err(StructuredMeshError::Invalid(
            "case must define inlet, outlet, two walls, and frontAndBack patches".into(),
        ));
    }
    Ok(PolyMesh {
        points,
        faces,
        owner,
        neighbour,
        patches,
    })
}
