use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// A single component value (scalar or one vector component).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScalarValue {
    pub value: f64,
}

impl From<f64> for ScalarValue {
    fn from(v: f64) -> Self {
        Self { value: v }
    }
}

/// A 3D vector value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VectorValue {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl VectorValue {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn components(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

/// Field type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    Scalar,
    Vector,
}

/// A cell-centered or face-centered field extracted from solver output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    /// Field name (e.g., "U", "phi", "Phi").
    pub name: String,
    /// Whether this is a scalar or vector field.
    pub field_type: FieldType,
    /// Number of values (cells or faces).
    pub n_values: usize,
    /// Scalar values, one per cell/face. For vectors, this holds the first component.
    pub values: Vec<f64>,
    /// Additional components for vector fields (y and z).
    #[serde(default)]
    pub extra_components: Vec<Vec<f64>>,
    /// Mesh identity hash for topology verification.
    #[serde(default)]
    pub mesh_hash: Option<String>,
    /// Cell/face identifiers if non-standard ordering.
    #[serde(default)]
    pub ids: Option<Vec<u32>>,
}

impl Field {
    /// Create a new scalar field.
    pub fn scalar(name: &str, values: Vec<f64>) -> Self {
        let n = values.len();
        Self {
            name: name.to_string(),
            field_type: FieldType::Scalar,
            n_values: n,
            values,
            extra_components: vec![],
            mesh_hash: None,
            ids: None,
        }
    }

    /// Create a new vector field from three component arrays.
    pub fn vector(name: &str, x: Vec<f64>, y: Vec<f64>, z: Vec<f64>) -> Self {
        let n = x.len();
        assert_eq!(y.len(), n);
        assert_eq!(z.len(), n);
        Self {
            name: name.to_string(),
            field_type: FieldType::Vector,
            n_values: n,
            values: x,
            extra_components: vec![y, z],
            mesh_hash: None,
            ids: None,
        }
    }

    /// Get all components for a given index.
    pub fn at(&self, i: usize) -> &[f64] {
        match self.field_type {
            FieldType::Scalar => std::slice::from_ref(&self.values[i]),
            FieldType::Vector => {
                // Build a temporary slice view — we store x in values, y/z in extra_components
                // For efficiency we return via a helper instead
                panic!("use at_components() for vector fields")
            }
        }
    }

    /// Get all component values at index i as a fixed-size array.
    pub fn at_components(&self, i: usize) -> [f64; 3] {
        match self.field_type {
            FieldType::Scalar => [self.values[i], 0.0, 0.0],
            FieldType::Vector => [
                self.values[i],
                self.extra_components[0][i],
                self.extra_components[1][i],
            ],
        }
    }

    /// Number of components (1 for scalar, 3 for vector).
    pub fn n_components(&self) -> usize {
        match self.field_type {
            FieldType::Scalar => 1,
            FieldType::Vector => 3,
        }
    }

    /// Check for NaN or infinity in any value.
    pub fn has_non_finite(&self) -> bool {
        if !self.values.iter().all(|v| v.is_finite()) {
            return true;
        }
        for comp in &self.extra_components {
            if !comp.iter().all(|v| v.is_finite()) {
                return true;
            }
        }
        false
    }

    /// Set the mesh hash.
    pub fn with_mesh_hash(mut self, hash: String) -> Self {
        self.mesh_hash = Some(hash);
        self
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Field({}, {} values, {:?})",
            self.name,
            self.n_values,
            self.field_type
        )
    }
}

/// Errors that can occur when parsing field data.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("IO error reading file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("Failed to parse field '{field}' from {path}: {detail}")]
    Parse {
        field: String,
        path: String,
        detail: String,
    },
    #[error("Field dimension mismatch in {path}: expected {expected}, got {got}")]
    DimensionMismatch {
        path: String,
        expected: usize,
        got: usize,
    },
    #[error("Unsupported format in {path}: {detail}")]
    UnsupportedFormat { path: String, detail: String },
}

/// Parse an OpenFOAM ASCII scalar field file (internalField only).
/// Handles the standard OF14 format:
/// ```text
/// internalField   nonuniform List<scalar>
/// N
/// ( v0 v1 v2 ... )
/// ```
/// Or uniform:
/// ```text
/// internalField   uniform value;
/// ```
pub fn parse_openfoam_scalar_internal(path: &Path) -> Result<Vec<f64>, ParseError> {
    let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let pstr = path.display().to_string();
    let content = std::fs::read_to_string(path)
        .map_err(|e| ParseError::Io { path: pstr.clone(), source: e })?;

    let start = content.find("internalField").ok_or_else(|| ParseError::Parse {
        field: fname.clone(), path: pstr.clone(), detail: "missing 'internalField' keyword".into(),
    })?;
    let after_kw = &content[start + "internalField".len()..];

    // Must check "nonuniform" BEFORE "uniform" since it contains that substring
    if is_nonuniform(after_kw) {
        // nonuniform List<scalar>: count appears before the data parentheses
        let (count, data_start) = extract_count_and_data(after_kw, &fname, &pstr)?;
        let data_section = &after_kw[data_start..];
        let data_end = find_matching_paren(data_section).ok_or_else(|| ParseError::Parse {
            field: fname.clone(), path: pstr.clone(), detail: "unmatched parenthesis in scalar data".into(),
        })?;

        let data_str = &data_section[..data_end];
        let values: Vec<f64> = data_str.split_whitespace()
            .filter_map(|t| t.parse::<f64>().ok())
            .collect();

        if values.len() != count {
            return Err(ParseError::DimensionMismatch {
                path: pstr, expected: count, got: values.len(),
            });
        }
        return Ok(values);
    } else if is_uniform(after_kw) {
        let rest = skip_to_keyword_value(after_kw);
        let num_str: String = rest.chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+' || *c == '.' || *c == 'e' || *c == 'E' || *c == ' ')
            .collect();
        let value: f64 = num_str.trim().parse().map_err(|_| ParseError::Parse {
            field: fname, path: pstr, detail: format!("cannot parse uniform value from '{num_str}'"),
        })?;
        return Ok(vec![value]);
    } else {
        return Err(ParseError::Parse {
            field: fname, path: pstr,
            detail: "unrecognized internalField format (neither uniform nor nonuniform)".into(),
        });
    }
}

/// Parse an OpenFOAM ASCII vector field file (internalField only).
/// Returns three component vectors. Handles OF14 format where each vector
/// is written as `(x y z)` on its own line within the outer list parens.
pub fn parse_openfoam_vector_internal(path: &Path) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), ParseError> {
    let fname = "U".to_string();
    let pstr = path.display().to_string();
    let content = std::fs::read_to_string(path)
        .map_err(|e| ParseError::Io { path: pstr.clone(), source: e })?;

    let start = content.find("internalField").ok_or_else(|| ParseError::Parse {
        field: fname.clone(), path: pstr.clone(), detail: "missing 'internalField' keyword".into(),
    })?;
    let after_kw = &content[start + "internalField".len()..];

    if is_nonuniform(after_kw) {
        let (count, data_start) = extract_count_and_data(after_kw, &fname, &pstr)?;
        let data_section = &after_kw[data_start..];
        let data_end = find_matching_paren(data_section).ok_or_else(|| ParseError::Parse {
            field: fname.clone(), path: pstr.clone(), detail: "unmatched parenthesis in vector data".into(),
        })?;

        let data_str = &data_section[..=data_end];
        // Strip parens that may be attached to numbers, then parse
        let cleaned: String = data_str.chars().filter(|c| *c != '(' && *c != ')').collect();
        let all_nums: Vec<f64> = cleaned.split_whitespace()
            .filter_map(|t| t.parse::<f64>().ok())
            .collect();

        if all_nums.len() != count * 3 {
            return Err(ParseError::DimensionMismatch {
                path: pstr, expected: count * 3, got: all_nums.len(),
            });
        }

        let mut x = Vec::with_capacity(count);
        let mut y = Vec::with_capacity(count);
        let mut z = Vec::with_capacity(count);
        for i in 0..count {
            x.push(all_nums[i * 3]);
            y.push(all_nums[i * 3 + 1]);
            z.push(all_nums[i * 3 + 2]);
        }
        Ok((x, y, z))
    } else if is_uniform(after_kw) {
        let rest = skip_to_keyword_value(after_kw);
        let paren_start = rest.find('(').unwrap_or(0);
        let paren_end = rest.find(')').unwrap_or(rest.len());
        let components: Vec<f64> = rest[paren_start + 1..paren_end]
            .split_whitespace()
            .filter_map(|t| t.parse::<f64>().ok())
            .collect();
        if components.len() != 3 {
            return Err(ParseError::Parse {
                field: fname, path: pstr,
                detail: format!("expected 3 components, got {}", components.len()),
            });
        }
        Ok((vec![components[0]], vec![components[1]], vec![components[2]]))
    } else {
        Err(ParseError::Parse {
            field: fname, path: pstr,
            detail: "unrecognized internalField format (neither uniform nor nonuniform)".into(),
        })
    }
}

/// Extract the element count and data-start position from text following "internalField".
/// OF14 format: `nonuniform List<type>\nN\n(\ndata...\n)`
/// Returns (count, byte_index_of_first_data_char_inside_parens).
fn extract_count_and_data(
    after_kw: &str,
    fname: &str,
    pstr: &str,
) -> Result<(usize, usize), ParseError> {
    // Find the first '(' which opens the data block.
    // The count is the integer that appears before it.
    let paren_idx = after_kw.find('(').ok_or_else(|| ParseError::Parse {
        field: fname.to_string(), path: pstr.to_string(),
        detail: "missing opening parenthesis for data".into(),
    })?;

    // Everything between "internalField" end and the data '(' contains the count
    let prefix = after_kw[..paren_idx].trim_end();
    // Extract the last integer in the prefix (skip type names like "List<scalar>")
    let count_str: String = prefix.chars().rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars().rev().collect();

    if count_str.is_empty() {
        return Err(ParseError::Parse {
            field: fname.to_string(), path: pstr.to_string(),
            detail: format!("cannot find element count in prefix: '{}'", prefix.trim()),
        });
    }

    let count: usize = count_str.parse().map_err(|_| ParseError::Parse {
        field: fname.to_string(), path: pstr.to_string(),
        detail: format!("invalid count '{count_str}'"),
    })?;

    // Data starts just inside the paren
    Ok((count, paren_idx + 1))
}

/// Find the index of the matching closing parenthesis.
fn find_matching_paren(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check if the text after "internalField" indicates a nonuniform field.
/// Must be checked before is_uniform since "nonuniform" contains "uniform".
fn is_nonuniform(after_kw: &str) -> bool {
    let trimmed = after_kw.trim_start();
    trimmed.starts_with("nonuniform")
}

/// Check if the text after "internalField" indicates a uniform field.
fn is_uniform(after_kw: &str) -> bool {
    let trimmed = after_kw.trim_start();
    // Must NOT start with "nonuniform" (already handled), but starts with "uniform"
    trimmed.starts_with("uniform") && !trimmed.starts_with("nonuniform")
}

/// Skip past the keyword to get the value portion.
/// For "uniform 5.0;" returns " 5.0;"
/// For "uniform (1 2 3);" returns " (1 2 3);"
fn skip_to_keyword_value(after_kw: &str) -> &str {
    let trimmed = after_kw.trim_start();
    if let Some(pos) = trimmed.find(' ') {
        &trimmed[pos..]
    } else {
        ""
    }
}
