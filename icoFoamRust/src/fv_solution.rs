//! Typed representation of system/fvSolution.

use crate::dict::{self, DictError};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FvSolution {
    pub solvers: Vec<SolverConfig>,
    pub piso: PisoConfig,
}

#[derive(Debug, Clone)]
pub struct SolverConfig {
    pub field: String,
    pub solver: String,
    pub preconditioner: Option<String>,
    pub smoother: Option<String>,
    pub tolerance: f64,
    pub rel_tol: f64,
}

#[derive(Debug, Clone)]
pub struct PisoConfig {
    pub momentum_predictor: bool,
    pub n_correctors: usize,
    pub n_non_orthogonal_correctors: usize,
    pub p_ref_cell: usize,
    pub p_ref_value: f64,
}

impl Default for PisoConfig {
    fn default() -> Self {
        Self {
            n_correctors: 2,
            momentum_predictor: true,
            n_non_orthogonal_correctors: 0,
            p_ref_cell: 0,
            p_ref_value: 0.0,
        }
    }
}

impl FvSolution {
    pub fn read(path: &Path) -> Result<Self, DictError> {
        let d = dict::read_file(path)?;
        let mut solvers = Vec::new();

        if let Some(solvers_block) = d.get_block("solvers") {
            for (field_name, val) in &solvers_block.entries {
                if let Some(block) = val.as_block() {
                    let solver = block
                        .get_str("solver")
                        .unwrap_or("smoothSolver")
                        .to_string();
                    let preconditioner = block.get_str("preconditioner").map(String::from);
                    let smoother = block.get_str("smoother").map(String::from);
                    let tolerance = block.get_f64("tolerance").unwrap_or(1e-7);
                    let rel_tol = block.get_f64("relTol").unwrap_or(0.0);
                    solvers.push(SolverConfig {
                        field: field_name.clone(),
                        solver,
                        preconditioner,
                        smoother,
                        tolerance,
                        rel_tol,
                    });
                }
            }
        }

        let piso = if let Some(piso_block) = d.get_block("PISO").or_else(|| d.get_block("PIMPLE")) {
            PisoConfig {
                momentum_predictor: !matches!(piso_block.get_str("momentumPredictor"), Some("no" | "false")),
                n_correctors: piso_block.get_usize("nCorrectors").unwrap_or(2),
                n_non_orthogonal_correctors: piso_block
                    .get_usize("nNonOrthogonalCorrectors")
                    .unwrap_or(0),
                p_ref_cell: piso_block.get_usize("pRefCell").unwrap_or(0),
                p_ref_value: piso_block.get_f64("pRefValue").unwrap_or(0.0),
            }
        } else {
            PisoConfig::default()
        };

        Ok(Self { solvers, piso })
    }

    pub fn solver_for(&self, field: &str) -> Option<&SolverConfig> {
        self.solvers.iter().find(|s| s.field == field)
    }
}
