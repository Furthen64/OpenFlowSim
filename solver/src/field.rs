//! Minimal ASCII volume-vector field reader for the first potentialFoam case.

use crate::mesh::Vector3;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct VectorPatch {
    pub name: String,
    pub type_name: String,
    pub fixed_value: Option<Vector3>,
}

#[derive(Debug, Clone)]
pub struct VolumeVectorField {
    pub dimensions: String,
    pub internal: Vec<Vector3>,
    pub boundary: Vec<VectorPatch>,
}

#[derive(Debug, thiserror::Error)]
pub enum FieldReadError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid vector field {path}: {detail}")]
    Invalid { path: PathBuf, detail: String },
}

impl VolumeVectorField {
    pub fn read(path: &Path) -> Result<Self, FieldReadError> {
        let text = fs::read_to_string(path).map_err(|source| FieldReadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let tokens = tokenize(&strip_comments(&text));
        let dimensions = token_after(&tokens, "dimensions")
            .and_then(|i| {
                (tokens.get(i).map(String::as_str) == Some("[")).then(|| {
                    let mut out = String::from("[");
                    let mut j = i + 1;
                    while j < tokens.len() && tokens[j] != "]" {
                        if out.len() > 1 {
                            out.push(' ');
                        }
                        out.push_str(&tokens[j]);
                        j += 1;
                    }
                    out.push(']');
                    out
                })
            })
            .unwrap_or_else(|| "[0 1 -1 0 0 0 0]".to_string());
        let dimensions = if dimensions == "[velocity]" {
            "[0 1 -1 0 0 0 0]".to_string()
        } else {
            dimensions
        };

        let internal_i = token_after(&tokens, "internalField")
            .ok_or_else(|| invalid(path, "missing internalField"))?;
        let internal = parse_vector_values(&tokens, internal_i, path)?;

        let boundary_i = token_after(&tokens, "boundaryField")
            .ok_or_else(|| invalid(path, "missing boundaryField"))?;
        let mut cursor = boundary_i;
        expect(&tokens, &mut cursor, "{", path)?;
        let mut boundary = Vec::new();
        while cursor < tokens.len() && tokens[cursor] != "}" {
            let patch_name = tokens[cursor].clone();
            cursor += 1;
            expect(&tokens, &mut cursor, "{", path)?;
            let start = cursor;
            let mut depth = 1usize;
            while cursor < tokens.len() && depth > 0 {
                match tokens[cursor].as_str() {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    _ => {}
                }
                cursor += 1;
            }
            if depth != 0 {
                return Err(invalid(path, format!("unterminated patch '{patch_name}'")));
            }
            let body = &tokens[start..cursor - 1];
            let patch_type = token_after(body, "type")
                .and_then(|i| body.get(i).cloned())
                .ok_or_else(|| invalid(path, format!("patch '{patch_name}' has no type")))?;
            let fixed_value = if matches!(patch_type.as_str(), "fixedValue" | "uniformFixedValue") {
                if patch_type == "uniformFixedValue" {
                    token_after(body, "uniformValue")
                        .map(|i| {
                            let mut j = i;
                            while j < body.len() && body[j] != "(" {
                                j += 1;
                            }
                            parse_vector(body, &mut j, path)
                        })
                        .transpose()?
                } else {
                    token_after(body, "value")
                        .map(|i| parse_vector_values(body, i, path))
                        .transpose()?
                        .and_then(|v| v.first().copied())
                }
            } else {
                None
            };
            boundary.push(VectorPatch {
                name: patch_name,
                type_name: patch_type,
                fixed_value,
            });
        }
        Ok(Self {
            dimensions,
            internal,
            boundary,
        })
    }
}

fn parse_vector_values(
    tokens: &[String],
    start: usize,
    path: &Path,
) -> Result<Vec<Vector3>, FieldReadError> {
    let mode = tokens
        .get(start)
        .map(String::as_str)
        .ok_or_else(|| invalid(path, "missing vector field value"))?;
    let mut cursor = start + 1;
    if mode == "uniform" {
        if tokens.get(cursor).map(String::as_str) == Some("constant") {
            cursor += 1;
        }
        return Ok(vec![parse_vector(tokens, &mut cursor, path)?]);
    }
    if mode != "nonuniform" {
        return Err(invalid(
            path,
            format!("unsupported vector field format '{mode}'"),
        ));
    }
    while cursor < tokens.len() && tokens[cursor].parse::<usize>().is_err() {
        cursor += 1;
    }
    let count = parse_usize(tokens.get(cursor), path)?;
    cursor += 1;
    expect(tokens, &mut cursor, "(", path)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(parse_vector(tokens, &mut cursor, path)?);
    }
    expect(tokens, &mut cursor, ")", path)?;
    Ok(values)
}

fn parse_vector(
    tokens: &[String],
    cursor: &mut usize,
    path: &Path,
) -> Result<Vector3, FieldReadError> {
    expect(tokens, cursor, "(", path)?;
    let x = parse_f64(tokens.get(*cursor), path)?;
    *cursor += 1;
    let y = parse_f64(tokens.get(*cursor), path)?;
    *cursor += 1;
    let z = parse_f64(tokens.get(*cursor), path)?;
    *cursor += 1;
    expect(tokens, cursor, ")", path)?;
    Ok(Vector3 { x, y, z })
}

fn parse_f64(token: Option<&String>, path: &Path) -> Result<f64, FieldReadError> {
    let token = token.ok_or_else(|| invalid(path, "unexpected end of vector"))?;
    token
        .parse()
        .map_err(|_| invalid(path, format!("invalid numeric value '{token}'")))
}

fn parse_usize(token: Option<&String>, path: &Path) -> Result<usize, FieldReadError> {
    let token = token.ok_or_else(|| invalid(path, "missing list size"))?;
    token
        .parse()
        .map_err(|_| invalid(path, format!("invalid list size '{token}'")))
}

fn token_after(tokens: &[String], key: &str) -> Option<usize> {
    tokens.iter().position(|token| token == key).map(|i| i + 1)
}

fn expect(
    tokens: &[String],
    cursor: &mut usize,
    expected: &str,
    path: &Path,
) -> Result<(), FieldReadError> {
    if tokens.get(*cursor).map(String::as_str) == Some(expected) {
        *cursor += 1;
        Ok(())
    } else {
        Err(invalid(
            path,
            format!("expected '{expected}' near token {}", *cursor),
        ))
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
    for c in input.chars() {
        if c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        } else if "(){}[];".contains(c) {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            out.push(c.to_string());
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn invalid(path: &Path, detail: impl Into<String>) -> FieldReadError {
    FieldReadError::Invalid {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}
