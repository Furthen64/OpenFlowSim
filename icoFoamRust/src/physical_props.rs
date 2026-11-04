//! Physical properties (nu) reader for incompressible cases.

use std::path::{Path, PathBuf};

use crate::dict::{self, DictError};

#[derive(Debug, thiserror::Error)]
pub enum PropertiesError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Dict(#[from] DictError),
    #[error("neither transportProperties nor physicalProperties found in constant/ of '{0}'")]
    MissingFile(PathBuf),
    #[error("'nu' not found in {0}")]
    MissingNu(PathBuf),
}

/// Transport/physical properties for an incompressible case.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalProperties {
    pub nu: f64,
}

impl PhysicalProperties {
    /// Read from `constant/transportProperties` or `constant/physicalProperties`.
    pub fn read(constant_dir: &Path) -> Result<Self, PropertiesError> {
        let candidates = [
            constant_dir.join("transportProperties"),
            constant_dir.join("physicalProperties"),
        ];
        let path = candidates
            .iter()
            .find(|p| p.exists())
            .ok_or_else(|| PropertiesError::MissingFile(constant_dir.to_path_buf()))?;

        let dict = dict::read_file(path)?;
        let nu = dict
            .get_f64("nu")
            .ok_or_else(|| PropertiesError::MissingNu(path.clone()))?;

        Ok(Self { nu })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_cavity_physical_properties() {
        let constant = Path::new(
            "/home/fat64/github/OpenFOAM-14/tutorials/legacy/incompressible/icoFoam/cavity/cavity/constant",
        );
        if !constant.exists() {
            return;
        }
        let props = PhysicalProperties::read(constant).unwrap();
        assert!((props.nu - 0.01).abs() < 1e-12);
    }

    #[test]
    fn missing_file_errors() {
        let result = PhysicalProperties::read(Path::new("/nonexistent/constant"));
        assert!(matches!(result, Err(PropertiesError::MissingFile(_))));
    }
}
