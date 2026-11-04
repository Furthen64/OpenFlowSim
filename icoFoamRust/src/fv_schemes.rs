//! Typed representation of system/fvSchemes.

use crate::dict::{self, DictError};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FvSchemes {
    pub ddt_default: String,
    pub grad_default: Vec<String>,
    pub div_schemes: Vec<(String, Vec<String>)>,
    pub laplacian_default: Vec<String>,
    pub interpolation_default: String,
}

impl Default for FvSchemes {
    fn default() -> Self {
        Self {
            ddt_default: "Euler".into(),
            grad_default: vec!["Gauss".into(), "linear".into()],
            div_schemes: Vec::new(),
            laplacian_default: vec!["Gauss".into(), "linear".into(), "orthogonal".into()],
            interpolation_default: "linear".into(),
        }
    }
}

impl FvSchemes {
    pub fn read(path: &Path) -> Result<Self, DictError> {
        let d = dict::read_file(path)?;
        let mut schemes = Self::default();

        if let Some(ddt) = d.get_block("ddtSchemes") {
            if let Some(v) = ddt.get("default") {
                schemes.ddt_default = v.as_str().unwrap_or("Euler").to_string();
            }
        }

        if let Some(grad) = d.get_block("gradSchemes") {
            if let Some(v) = grad.get("default") {
                schemes.grad_default = v.tokens().iter().cloned().collect();
            }
        }

        if let Some(div) = d.get_block("divSchemes") {
            for (key, val) in &div.entries {
                if key == "default" {
                    continue;
                }
                schemes.div_schemes.push((key.clone(), val.tokens().iter().cloned().collect()));
            }
        }

        if let Some(lap) = d.get_block("laplacianSchemes") {
            if let Some(v) = lap.get("default") {
                schemes.laplacian_default = v.tokens().iter().cloned().collect();
            }
        }

        if let Some(interp) = d.get_block("interpolationSchemes") {
            if let Some(v) = interp.get("default") {
                schemes.interpolation_default = v.as_str().unwrap_or("linear").to_string();
            }
        }

        Ok(schemes)
    }

    pub fn div_scheme_for(&self, expr: &str) -> Option<&Vec<String>> {
        self.div_schemes
            .iter()
            .find(|(k, _)| k == expr)
            .map(|(_, v)| v)
    }
}
