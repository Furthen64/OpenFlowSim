//! Boundary conditions shared by the finite-volume operators and solver.

use crate::scalar_field::{BoundaryPatch, FieldError};
use crate::Vector3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryCondition<T> {
    FixedValue(T),
    ZeroGradient,
    NoSlip,
    SymmetryPlane,
    Empty,
    FixedFluxPressure(f64),
    InletOutlet { inlet_value: T },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryConditions<T> {
    pub patches: Vec<BoundaryCondition<T>>,
    /// Optional per-face values, used when the field file specifies a
    /// nonuniform patch value. Uniform values are stored in `patches`.
    pub patch_values: Vec<Option<Vec<T>>>,
    /// Optional per-face normal gradients, used by fixedFluxPressure.
    pub patch_gradients: Vec<Option<Vec<f64>>>,
}

impl<T> BoundaryConditions<T> {
    pub fn new(patches: Vec<BoundaryCondition<T>>) -> Self {
        let n = patches.len();
        Self {
            patches,
            patch_values: std::iter::repeat_with(|| None).take(n).collect(),
            patch_gradients: std::iter::repeat_with(|| None).take(n).collect(),
        }
    }
}

impl BoundaryConditions<f64> {
    /// Build scalar BCs from a parsed OpenFOAM field. `fixedFluxPressure`
    /// defaults to zero until the pressure constraint updates its gradient.
    pub fn from_scalar_patches(patches: &[BoundaryPatch]) -> Result<Self, FieldError> {
        let mut conditions = Vec::with_capacity(patches.len());
        let mut values = Vec::with_capacity(patches.len());
        for patch in patches {
            let vals = patch.values.as_ref();
            let scalar = vals.and_then(|v| v.first()).copied().unwrap_or(0.0);
            let condition = match patch.bc_type.as_str() {
                "fixedValue" | "uniformFixedValue" => BoundaryCondition::FixedValue(scalar),
                "zeroGradient" | "calculated" => BoundaryCondition::ZeroGradient,
                "noSlip" => BoundaryCondition::NoSlip,
                "symmetry" | "symmetryPlane" => BoundaryCondition::SymmetryPlane,
                "empty" => BoundaryCondition::Empty,
                "fixedFluxPressure" => BoundaryCondition::FixedFluxPressure(0.0),
                "inletOutlet" => BoundaryCondition::InletOutlet {
                    inlet_value: scalar,
                },
                other => {
                    return Err(FieldError::Parse {
                        path: "<boundary conditions>".into(),
                        detail: format!(
                            "unsupported scalar boundary condition '{other}' on patch '{}'",
                            patch.name
                        ),
                    })
                }
            };
            // Keep only genuinely nonuniform values. Uniform lists are already
            // represented by the condition's scalar value.
            let face_values = vals.filter(|v| v.len() > 1).cloned();
            conditions.push(condition);
            values.push(face_values);
        }
        Ok(Self {
            patches: conditions,
            patch_values: values,
            patch_gradients: std::iter::repeat_with(|| None)
                .take(patches.len())
                .collect(),
        })
    }

    pub fn face_value(
        &self,
        patch: usize,
        local_face: usize,
        owner_value: f64,
        outward_flux: f64,
    ) -> f64 {
        if let Some(Some(values)) = self.patch_values.get(patch) {
            if let Some(value) = values.get(local_face) {
                return *value;
            }
        }
        match self
            .patches
            .get(patch)
            .copied()
            .unwrap_or(BoundaryCondition::ZeroGradient)
        {
            BoundaryCondition::FixedValue(value) => value,
            BoundaryCondition::InletOutlet { inlet_value } => {
                if outward_flux < 0.0 {
                    inlet_value
                } else {
                    owner_value
                }
            }
            BoundaryCondition::NoSlip => 0.0,
            BoundaryCondition::SymmetryPlane
            | BoundaryCondition::Empty
            | BoundaryCondition::ZeroGradient
            | BoundaryCondition::FixedFluxPressure(_) => owner_value,
        }
    }

    pub fn is_dirichlet(&self, patch: usize) -> Option<f64> {
        match self.patches.get(patch).copied()? {
            BoundaryCondition::FixedValue(value) => Some(value),
            BoundaryCondition::NoSlip => Some(0.0),
            _ => None,
        }
    }

    pub fn dirichlet_face_value(&self, patch: usize, local_face: usize) -> Option<f64> {
        let base = self.is_dirichlet(patch)?;
        self.patch_values
            .get(patch)
            .and_then(Option::as_ref)
            .and_then(|values| values.get(local_face))
            .copied()
            .or(Some(base))
    }

    pub fn fixed_flux_gradient(&self, patch: usize) -> Option<f64> {
        match self.patches.get(patch).copied()? {
            BoundaryCondition::FixedFluxPressure(gradient) => Some(gradient),
            _ => None,
        }
    }

    pub fn fixed_flux_gradient_at(&self, patch: usize, local_face: usize) -> Option<f64> {
        self.patch_gradients
            .get(patch)
            .and_then(Option::as_ref)
            .and_then(|gradients| gradients.get(local_face))
            .copied()
            .or_else(|| self.fixed_flux_gradient(patch))
    }

    pub fn set_fixed_flux_gradient(&mut self, patch: usize, gradient: f64) -> bool {
        match self.patches.get_mut(patch) {
            Some(condition @ BoundaryCondition::FixedFluxPressure(_)) => {
                *condition = BoundaryCondition::FixedFluxPressure(gradient);
                if let Some(values) = self.patch_gradients.get_mut(patch) {
                    *values = None;
                }
                true
            }
            _ => false,
        }
    }

    pub fn set_fixed_flux_gradient_at(
        &mut self,
        patch: usize,
        local_face: usize,
        gradient: f64,
    ) -> bool {
        let Some(condition) = self.patches.get(patch) else {
            return false;
        };
        let BoundaryCondition::FixedFluxPressure(base) = *condition else {
            return false;
        };
        let Some(values) = self.patch_gradients.get_mut(patch) else {
            return false;
        };
        let gradients = values.get_or_insert_with(|| vec![base; local_face + 1]);
        if gradients.len() <= local_face {
            gradients.resize(local_face + 1, base);
        }
        gradients[local_face] = gradient;
        true
    }
}

impl BoundaryConditions<Vector3> {
    /// Explicit vector conditions for solver-created fields. The field parser
    /// currently handles scalar values; callers provide vector patch values.
    pub fn from_vector_patches(patches: &[foamcore_solver::field::VectorPatch]) -> Self {
        let conditions = patches
            .iter()
            .map(|patch| match patch.type_name.as_str() {
                "fixedValue" | "uniformFixedValue" => {
                    BoundaryCondition::FixedValue(patch.fixed_value.unwrap_or(Vector3::zero()))
                }
                "noSlip" => BoundaryCondition::NoSlip,
                "symmetry" | "symmetryPlane" => BoundaryCondition::SymmetryPlane,
                "empty" => BoundaryCondition::Empty,
                "inletOutlet" => BoundaryCondition::InletOutlet {
                    inlet_value: patch.fixed_value.unwrap_or(Vector3::zero()),
                },
                _ => BoundaryCondition::ZeroGradient,
            })
            .collect();
        Self::new(conditions)
    }

    pub fn face_value(
        &self,
        patch: usize,
        local_face: usize,
        owner: Vector3,
        outward_flux: f64,
    ) -> Vector3 {
        if let Some(Some(values)) = self.patch_values.get(patch) {
            if let Some(value) = values.get(local_face) {
                return *value;
            }
        }
        match self
            .patches
            .get(patch)
            .copied()
            .unwrap_or(BoundaryCondition::ZeroGradient)
        {
            BoundaryCondition::FixedValue(value) => value,
            BoundaryCondition::NoSlip => Vector3::zero(),
            BoundaryCondition::InletOutlet { inlet_value } => {
                if outward_flux < 0.0 {
                    inlet_value
                } else {
                    owner
                }
            }
            BoundaryCondition::SymmetryPlane => {
                let _ = local_face;
                owner
            }
            BoundaryCondition::Empty
            | BoundaryCondition::ZeroGradient
            | BoundaryCondition::FixedFluxPressure(_) => owner,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_patch_mapping_preserves_values_and_types() {
        let patches = vec![
            BoundaryPatch {
                name: "inlet".into(),
                bc_type: "fixedValue".into(),
                values: Some(vec![2.0]),
            },
            BoundaryPatch {
                name: "outlet".into(),
                bc_type: "zeroGradient".into(),
                values: None,
            },
            BoundaryPatch {
                name: "wall".into(),
                bc_type: "fixedFluxPressure".into(),
                values: None,
            },
        ];
        let bcs = BoundaryConditions::from_scalar_patches(&patches).unwrap();
        assert_eq!(bcs.face_value(0, 0, 9.0, 1.0), 2.0);
        assert_eq!(bcs.face_value(1, 0, 9.0, 1.0), 9.0);
        assert_eq!(bcs.fixed_flux_gradient(2), Some(0.0));
        let mut bcs = bcs;
        assert!(bcs.set_fixed_flux_gradient_at(2, 1, 3.5));
        assert_eq!(bcs.fixed_flux_gradient_at(2, 0), Some(0.0));
        assert_eq!(bcs.fixed_flux_gradient_at(2, 1), Some(3.5));
        assert!(!bcs.set_fixed_flux_gradient_at(1, 1, 3.5));
    }

    #[test]
    fn inlet_outlet_switches_on_flux_sign() {
        let bcs = BoundaryConditions::<f64>::new(vec![BoundaryCondition::InletOutlet {
            inlet_value: 4.0,
        }]);
        assert_eq!(bcs.face_value(0, 0, 1.5, -0.1), 4.0);
        assert_eq!(bcs.face_value(0, 0, 1.5, 0.1), 1.5);
    }
}
