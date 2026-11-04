//! Rust implementation of OpenFOAM icoFoam (transient incompressible laminar Navier-Stokes).

pub mod control_dict;
pub mod dict;
pub mod fv_schemes;
pub mod fv_solution;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CaseError {
    #[error("missing system directory in case '{0}'")]
    MissingSystem(PathBuf),
    #[error(transparent)]
    Dict(#[from] dict::DictError),
}

pub struct CaseSetup {
    pub control: control_dict::ControlDict,
    pub schemes: fv_schemes::FvSchemes,
    pub solution: fv_solution::FvSolution,
}

impl CaseSetup {
    pub fn read(case_dir: &std::path::Path) -> Result<Self, CaseError> {
        let system = case_dir.join("system");
        if !system.is_dir() {
            return Err(CaseError::MissingSystem(case_dir.to_path_buf()));
        }

        let control = control_dict::ControlDict::read(&system.join("controlDict"))?;
        let schemes = fv_schemes::FvSchemes::read(&system.join("fvSchemes"))?;
        let solution = fv_solution::FvSolution::read(&system.join("fvSolution"))?;

        Ok(Self {
            control,
            schemes,
            solution,
        })
    }
}
