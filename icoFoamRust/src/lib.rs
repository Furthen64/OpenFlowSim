//! Rust implementation of OpenFOAM icoFoam (transient incompressible laminar Navier-Stokes).

pub mod bc;
pub mod control_dict;
pub mod dict;
pub mod fv_matrix;
pub mod fv_ops;
pub mod fv_schemes;
pub mod fv_solution;
pub mod physical_props;
pub mod piso;
pub mod scalar_field;
pub mod time_dirs;

/// Re-export mesh types from the core solver crate for convenience.
pub use foamcore_solver::mesh::{MeshGeometry, PolyMesh, Vector3};

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CaseError {
    #[error("missing system directory in case '{0}'")]
    MissingSystem(PathBuf),
    #[error(transparent)]
    Dict(#[from] dict::DictError),
    #[error(transparent)]
    Properties(#[from] physical_props::PropertiesError),
}

pub struct CaseSetup {
    pub control: control_dict::ControlDict,
    pub schemes: fv_schemes::FvSchemes,
    pub solution: fv_solution::FvSolution,
    pub properties: physical_props::PhysicalProperties,
}

impl CaseSetup {
    pub fn read(case_dir: &Path) -> Result<Self, CaseError> {
        let system = case_dir.join("system");
        if !system.is_dir() {
            return Err(CaseError::MissingSystem(case_dir.to_path_buf()));
        }

        let constant = case_dir.join("constant");
        let control = control_dict::ControlDict::read(&system.join("controlDict"))?;
        let schemes = fv_schemes::FvSchemes::read(&system.join("fvSchemes"))?;
        let solution = fv_solution::FvSolution::read(&system.join("fvSolution"))?;
        let properties = physical_props::PhysicalProperties::read(&constant)?;

        Ok(Self {
            control,
            schemes,
            solution,
            properties,
        })
    }
}
