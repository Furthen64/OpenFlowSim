//! OpenFOAM scalar field reader/writer (volScalarField and surfaceScalarField).

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum FieldError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {detail}")]
    Parse { path: PathBuf, detail: String },
    #[error("failed to write {path}: {source}")]
    WriteIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A boundary condition patch on a scalar field.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryPatch {
    pub name: String,
    pub bc_type: String,
    /// Explicit values for the patch (fixedValue, calculated, etc.).
    /// None when the BC type implies no stored values (zeroGradient, empty without data).
    pub values: Option<Vec<f64>>,
}

/// A scalar field stored at cell centres (vol) or face centres (surface).
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarField {
    pub dimensions: String,
    pub internal: Vec<f64>,
    pub boundary: Vec<BoundaryPatch>,
}

impl ScalarField {
    /// Read a scalar field file from an OpenFOAM case directory.
    pub fn read(path: &Path) -> Result<Self, FieldError> {
        let text = fs::read_to_string(path).map_err(|source| FieldError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text, path)
    }

    /// Write this field in OpenFOAM ASCII format.
    pub fn write(&self, path: &Path, class_name: &str, location: &str, object: &str) -> Result<(), FieldError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| FieldError::WriteIo {
                path: path.to_path_buf(),
                source,
            })?;
        }
        let content = self.format(class_name, location, object);
        fs::write(path, content).map_err(|source| FieldError::WriteIo {
            path: path.to_path_buf(),
            source,
        })
    }

    fn format(&self, class_name: &str, location: &str, object: &str) -> String {
        let mut s = String::with_capacity(1024 + self.internal.len() * 20);
        s.push_str("/*--------------------------------*- C++ -*----------------------------------*/\n");
        s.push_str("FoamFile\n{\n");
        s.push_str("    version     2.0;\n");
        s.push_str("    format      ascii;\n");
        s.push_str("    class       ");
        s.push_str(class_name);
        s.push_str(";\n");
        s.push_str("    location    \"");
        s.push_str(location);
        s.push_str("\";\n");
        s.push_str("    object      ");
        s.push_str(object);
        s.push_str(";\n}\n");
        s.push_str("// * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * * //\n\n");
        s.push_str("dimensions      ");
        s.push_str(&self.dimensions);
        s.push_str(";\n\n");

        if self.internal.iter().all(|v| (v - &self.internal[0]).abs() < f64::EPSILON) {
            s.push_str("internalField   uniform ");
            s.push_str(&fmt_scalar(self.internal[0]));
            s.push_str(";\n");
        } else {
            s.push_str("internalField   nonuniform List<scalar>\n");
            s.push_str(&self.internal.len().to_string());
            s.push_str("\n(\n");
            for v in &self.internal {
                s.push_str(&fmt_scalar(*v));
                s.push('\n');
            }
            s.push_str(")\n;\n");
        }

        s.push_str("boundaryField\n{\n");
        for patch in &self.boundary {
            s.push_str("    ");
            s.push_str(&patch.name);
            s.push_str("\n    {\n");
            s.push_str("        type            ");
            s.push_str(&patch.bc_type);
            s.push_str(";\n");
            if let Some(vals) = &patch.values {
                if vals.is_empty() {
                    s.push_str("        value           nonuniform List<scalar> 0();\n");
                } else if vals.iter().all(|v| (v - &vals[0]).abs() < f64::EPSILON) {
                    s.push_str("        value           uniform ");
                    s.push_str(&fmt_scalar(vals[0]));
                    s.push_str(";\n");
                } else {
                    s.push_str("        value           nonuniform List<scalar>\n");
                    s.push_str(&vals.len().to_string());
                    s.push_str("\n        (\n");
                    for v in vals {
                        s.push(' ');
                        s.push_str(&fmt_scalar(*v));
                        s.push('\n');
                    }
                    s.push_str("        )\n        ;\n");
                }
            }
            s.push_str("    }\n");
        }
        s.push_str("}\n\n// ************************************************************************* //\n");
        s
    }

    fn parse(text: &str, path: &Path) -> Result<Self, FieldError> {
        let stripped = strip_comments(text);
        let tokens = tokenize(&stripped);
        let mut pos = 0;

        // Skip FoamFile header
        if tokens.first().is_some_and(|t| t == "FoamFile") {
            pos += 1;
            pos = skip_block(&tokens, pos)?;
        }

        // dimensions — brackets are separate tokens, reconstruct without extra spaces
        expect_token(&tokens, &mut pos, "dimensions", path)?;
        let dims_start = pos;
        while pos < tokens.len() && tokens[pos] != ";" {
            pos += 1;
        }
        let dimensions = format_dimensions(&tokens[dims_start..pos]);
        pos += 1; // skip ';'

        // internalField
        expect_token(&tokens, &mut pos, "internalField", path)?;
        let internal = parse_internal_field(&tokens, &mut pos, path)?;

        // boundaryField
        expect_token(&tokens, &mut pos, "boundaryField", path)?;
        let boundary = parse_boundary_field(&tokens, &mut pos, path)?;

        Ok(Self { dimensions, internal, boundary })
    }
}

fn parse_internal_field(tokens: &[String], pos: &mut usize, path: &Path) -> Result<Vec<f64>, FieldError> {
    match tokens[*pos].as_str() {
        "uniform" => {
            *pos += 1;
            let val: f64 = tokens[*pos]
                .parse()
                .map_err(|_| FieldError::Parse {
                    path: path.to_path_buf(),
                    detail: format!("invalid scalar '{}'", tokens[*pos]),
                })?;
            *pos += 1;
            consume_semicolon(tokens, pos);
            Ok(vec![val])
        }
        "nonuniform" => {
            *pos += 1;
            // Expect List<scalar> or similar type tag
            if tokens.get(*pos).is_some_and(|t| t.starts_with("List")) {
                *pos += 1;
            }
            parse_list_values(tokens, pos, path, "in list")
        }
        other => Err(FieldError::Parse {
            path: path.to_path_buf(),
            detail: format!("unexpected internalField keyword '{other}'"),
        }),
    }
}

fn parse_boundary_field(tokens: &[String], pos: &mut usize, path: &Path) -> Result<Vec<BoundaryPatch>, FieldError> {
    expect_token(tokens, pos, "{", path)?;
    let mut patches = Vec::new();
    while tokens.get(*pos).is_some_and(|t| t != "}") {
        let name = tokens[*pos].clone();
        *pos += 1;
        expect_token(tokens, pos, "{", path)?;

        let mut bc_type = String::new();
        let mut values: Option<Vec<f64>> = None;

        while tokens.get(*pos).is_some_and(|t| t != "}") {
            let key = tokens[*pos].clone();
            *pos += 1;
            match key.as_str() {
                "type" => {
                    bc_type = tokens[*pos].clone();
                    *pos += 1;
                    consume_semicolon(tokens, pos);
                }
                "value" => {
                    values = Some(parse_patch_value(tokens, pos, path)?);
                }
                _ => {
                    // Skip unknown entries (consume until ';')
                    while tokens.get(*pos).is_some_and(|t| t != ";" && t != "}") {
                        *pos += 1;
                    }
                    consume_semicolon(tokens, pos);
                }
            }
        }
        expect_token(tokens, pos, "}", path)?;
        patches.push(BoundaryPatch { name, bc_type, values });
    }
    expect_token(tokens, pos, "}", path)?;
    Ok(patches)
}

fn parse_patch_value(tokens: &[String], pos: &mut usize, path: &Path) -> Result<Vec<f64>, FieldError> {
    match tokens[*pos].as_str() {
        "uniform" => {
            *pos += 1;
            let val: f64 = tokens[*pos]
                .parse()
                .map_err(|_| FieldError::Parse {
                    path: path.to_path_buf(),
                    detail: format!("invalid scalar '{}'", tokens[*pos]),
                })?;
            *pos += 1;
            consume_semicolon(tokens, pos);
            Ok(vec![val])
        }
        "nonuniform" => {
            *pos += 1;
            if tokens.get(*pos).is_some_and(|t| t.starts_with("List")) {
                *pos += 1;
            }
            parse_list_values(tokens, pos, path, "in patch value")
        }
        other => Err(FieldError::Parse {
            path: path.to_path_buf(),
            detail: format!("unexpected patch value keyword '{other}'"),
        }),
    }
}

/// Parse a list of scalar values after the type tag has been consumed.
/// Handles both `N ( v1 v2 ... ) ;` and the tokenizer's merged `N();` form
/// for empty lists (where `(` attaches to the preceding digit token).
fn parse_list_values(tokens: &[String], pos: &mut usize, path: &Path, context: &str) -> Result<Vec<f64>, FieldError> {
    let tok = tokens[*pos].clone();
    // Check if this is an empty-list shorthand like "0()" produced by the tokenizer
    if let Some(stripped) = tok.strip_suffix("()") {
        let count: usize = stripped.parse().map_err(|_| FieldError::Parse {
            path: path.to_path_buf(),
            detail: format!("invalid list size '{tok}' {context}"),
        })?;
        *pos += 1;
        consume_semicolon(tokens, pos);
        debug_assert_eq!(count, 0, "non-zero count with () shorthand");
        return Ok(Vec::new());
    }

    let count: usize = tok.parse().map_err(|_| FieldError::Parse {
        path: path.to_path_buf(),
        detail: format!("invalid list size '{tok}' {context}"),
    })?;
    *pos += 1;
    expect_token(tokens, pos, "(", path)?;
    let mut vals = Vec::with_capacity(count);
    for _ in 0..count {
        let v: f64 = tokens[*pos]
            .parse()
            .map_err(|_| FieldError::Parse {
                path: path.to_path_buf(),
                detail: format!("invalid scalar '{}' {context}", tokens[*pos]),
            })?;
        vals.push(v);
        *pos += 1;
    }
    expect_token(tokens, pos, ")", path)?;
    consume_semicolon(tokens, pos);
    Ok(vals)
}

fn expect_token(tokens: &[String], pos: &mut usize, expected: &str, path: &Path) -> Result<(), FieldError> {
    match tokens.get(*pos).map(String::as_str) {
        Some(t) if t == expected => {
            *pos += 1;
            Ok(())
        }
        other => Err(FieldError::Parse {
            path: path.to_path_buf(),
            detail: format!("expected '{expected}', found '{other:?}' at token {pos}"),
        }),
    }
}

fn consume_semicolon(tokens: &[String], pos: &mut usize) {
    if tokens.get(*pos).is_some_and(|t| t == ";") {
        *pos += 1;
    }
}

fn skip_block(tokens: &[String], mut pos: usize) -> Result<usize, FieldError> {
    // pos should be at '{'
    if tokens.get(pos).is_some_and(|t| t == "{") {
        pos += 1;
        let mut depth = 1usize;
        while pos < tokens.len() && depth > 0 {
            match tokens[pos].as_str() {
                "{" => depth += 1,
                "}" => depth -= 1,
                _ => {}
            }
            pos += 1;
        }
    }
    Ok(pos)
}

/// Reconstruct a dimensions string from tokens, avoiding spaces around brackets.
/// Tokens like `[`, `0`, `2`, `-1`, ..., `]` become `[0 2 -1 ...]`.
fn format_dimensions(tokens: &[String]) -> String {
    let mut s = String::new();
    for (i, tok) in tokens.iter().enumerate() {
        match tok.as_str() {
            "[" => s.push('['),
            "]" => s.push(']'),
            _ => {
                if !s.is_empty() && !s.ends_with('[') {
                    s.push(' ');
                }
                s.push_str(tok);
            }
        }
        let _ = i;
    }
    s
}

fn fmt_scalar(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v:.9e}")
    }
}

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            i += 1;
        } else if "(){}[];".contains(c) {
            if c == '(' && !word.is_empty() {
                word.push(c);
                i += 1;
                let mut depth = 1usize;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    word.push(chars[i]);
                    i += 1;
                }
            } else {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                out.push(c.to_string());
                i += 1;
            }
        } else {
            word.push(c);
            i += 1;
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_field(text: &str) -> ScalarField {
        ScalarField::parse(text, Path::new("test")).unwrap()
    }

    #[test]
    fn uniform_internal_zero_gradient_bc() {
        let text = r#"
FoamFile
{
    format      ascii;
    class       volScalarField;
    location    "0";
    object      p;
}
dimensions      [kinematicPressure];
internalField   uniform 0;
boundaryField
{
    movingWall { type zeroGradient; }
    fixedWalls { type zeroGradient; }
    frontAndBack { type empty; }
}
"#;
        let f = parse_field(text);
        assert_eq!(f.dimensions, "[kinematicPressure]");
        assert_eq!(f.internal, vec![0.0]);
        assert_eq!(f.boundary.len(), 3);
        assert_eq!(f.boundary[0].name, "movingWall");
        assert_eq!(f.boundary[0].bc_type, "zeroGradient");
        assert!(f.boundary[0].values.is_none());
        assert_eq!(f.boundary[2].bc_type, "empty");
    }

    #[test]
    fn nonuniform_internal_fixed_value_bc() {
        let text = r#"
FoamFile
{
    format      ascii;
    class       volScalarField;
    location    "0";
    object      T;
}
dimensions      [0 0 1 0 0 0 0];
internalField   nonuniform List<scalar>
4
(
300.0
301.0
302.0
303.0
)
;
boundaryField
{
    hotWall
    {
        type            fixedValue;
        value           uniform 350;
    }
    coldWall
    {
        type            zeroGradient;
    }
}
"#;
        let f = parse_field(text);
        assert_eq!(f.internal, vec![300.0, 301.0, 302.0, 303.0]);
        assert_eq!(f.boundary[0].name, "hotWall");
        assert_eq!(f.boundary[0].bc_type, "fixedValue");
        assert_eq!(f.boundary[0].values, Some(vec![350.0]));
        assert!(f.boundary[1].values.is_none());
    }

    #[test]
    fn surface_scalar_empty_patch() {
        let text = r#"
FoamFile
{
    format      ascii;
    class       surfaceScalarField;
    location    "0";
    object      phi;
}
dimensions      [0 3 -1 0 0 0 0];
internalField   nonuniform List<scalar>
4
(
1.0
-2.0
3.0
-4.0
)
;
boundaryField
{
    frontAndBack
    {
        type            empty;
        value           nonuniform List<scalar> 0();
    }
    wall
    {
        type            calculated;
        value           uniform 0;
    }
}
"#;
        let f = parse_field(text);
        assert_eq!(f.internal.len(), 4);
        assert_eq!(f.internal[1], -2.0);
        assert_eq!(f.boundary[0].bc_type, "empty");
        assert_eq!(f.boundary[0].values, Some(vec![]));
        assert_eq!(f.boundary[1].bc_type, "calculated");
        assert_eq!(f.boundary[1].values, Some(vec![0.0]));
    }

    #[test]
    fn roundtrip_write_read() {
        let field = ScalarField {
            dimensions: "[0 2 -1 0 0 0 0]".to_string(),
            internal: vec![1.5, 2.5, 3.5],
            boundary: vec![
                BoundaryPatch { name: "wall".into(), bc_type: "zeroGradient".into(), values: None },
                BoundaryPatch { name: "inlet".into(), bc_type: "fixedValue".into(), values: Some(vec![10.0]) },
            ],
        };
        let content = field.format("volScalarField", "0", "p");
        let reparsed = ScalarField::parse(&content, Path::new("rt")).unwrap();
        assert_eq!(reparsed.dimensions, field.dimensions);
        assert_eq!(reparsed.internal, field.internal);
        assert_eq!(reparsed.boundary.len(), 2);
        assert_eq!(reparsed.boundary[1].values, Some(vec![10.0]));
    }

    #[test]
    fn read_cavity_p() {
        let path = Path::new(
            "/home/fat64/github/OpenFOAM-14/tutorials/legacy/incompressible/icoFoam/cavity/cavity/0/p",
        );
        if !path.exists() {
            return;
        }
        let f = ScalarField::read(path).unwrap();
        assert_eq!(f.internal, vec![0.0]);
        assert_eq!(f.boundary.len(), 3);
    }

    #[test]
    fn read_cavity_phi() {
        let path = Path::new(
            "/home/fat64/github/OpenFOAM-14/tutorials/legacy/incompressible/icoFoam/cavity/cavity/0/phi",
        );
        if !path.exists() {
            return;
        }
        let f = ScalarField::read(path).unwrap();
        // phi should have nFaces entries in internal field
        assert!(!f.internal.is_empty());
    }
}
