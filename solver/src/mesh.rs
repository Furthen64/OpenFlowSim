//! Reader for the ASCII OpenFOAM `constant/polyMesh` topology files.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    pub fn dot(self, rhs: Self) -> f64 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    pub fn cross(self, rhs: Self) -> Self {
        Self {
            x: self.y * rhs.z - self.z * rhs.y,
            y: self.z * rhs.x - self.x * rhs.z,
            z: self.x * rhs.y - self.y * rhs.x,
        }
    }

    pub fn magnitude(self) -> f64 {
        self.dot(self).sqrt()
    }
}

impl std::ops::Add for Vector3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl std::ops::AddAssign for Vector3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub for Vector3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl std::ops::Mul<f64> for Vector3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

impl std::ops::Div<f64> for Vector3 {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
            z: self.z / rhs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryPatch {
    pub name: String,
    pub patch_type: String,
    pub start_face: usize,
    pub face_count: usize,
}

/// OpenFOAM's face topology. Internal faces occupy `[0, neighbour.len())`;
/// the remaining faces are boundary faces described by `patches`.
#[derive(Debug, Clone)]
pub struct PolyMesh {
    pub points: Vec<Vector3>,
    pub faces: Vec<Vec<usize>>,
    pub owner: Vec<usize>,
    pub neighbour: Vec<usize>,
    pub patches: Vec<BoundaryPatch>,
}

#[derive(Debug, Clone, Copy)]
pub struct FaceGeometry {
    /// Area vector oriented out of the owner cell.
    pub area: Vector3,
    pub centre: Vector3,
}

#[derive(Debug, Clone, Copy)]
pub struct CellGeometry {
    pub centre: Vector3,
    pub volume: f64,
}

#[derive(Debug, Clone)]
pub struct MeshGeometry {
    pub faces: Vec<FaceGeometry>,
    pub cells: Vec<CellGeometry>,
}

#[derive(Debug, thiserror::Error)]
pub enum MeshReadError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid OpenFOAM mesh file {path}: {detail}")]
    Invalid { path: PathBuf, detail: String },
}

impl PolyMesh {
    /// Read ASCII OpenFOAM topology from `<case>/constant/polyMesh`.
    pub fn read_case(case_dir: &Path) -> Result<Self, MeshReadError> {
        let mesh_dir = case_dir.join("constant").join("polyMesh");
        let points = read_points(&mesh_dir.join("points"))?;
        let faces = read_faces(&mesh_dir.join("faces"))?;
        let owner = read_labels(&mesh_dir.join("owner"))?;
        let neighbour = read_labels(&mesh_dir.join("neighbour"))?;
        let patches = read_boundary(&mesh_dir.join("boundary"))?;
        let mesh = Self {
            points,
            faces,
            owner,
            neighbour,
            patches,
        };
        mesh.validate(&mesh_dir)?;
        Ok(mesh)
    }

    pub fn cell_count(&self) -> usize {
        self.owner
            .iter()
            .chain(&self.neighbour)
            .copied()
            .max()
            .map_or(0, |n| n + 1)
    }

    pub fn internal_face_count(&self) -> usize {
        self.neighbour.len()
    }

    /// Compute polygon face geometry and cell volumes/centres using face
    /// pyramids. Face winding follows OpenFOAM's owner-outward convention.
    pub fn geometry(&self) -> Result<MeshGeometry, MeshReadError> {
        let mut face_geometry = Vec::with_capacity(self.faces.len());
        for (face_i, face) in self.faces.iter().enumerate() {
            let mut area = Vector3::zero();
            for i in 0..face.len() {
                let a = self.points[face[i]];
                let b = self.points[face[(i + 1) % face.len()]];
                area += a.cross(b) * 0.5;
            }
            let area_magnitude = area.magnitude();
            if area_magnitude <= f64::EPSILON {
                return Err(MeshReadError::Invalid {
                    path: PathBuf::from("constant/polyMesh/faces"),
                    detail: format!("face {face_i} has zero area"),
                });
            }
            let normal = area / area_magnitude;
            let anchor = self.points[face[0]];
            let mut weighted_centre = Vector3::zero();
            let mut signed_area = 0.0;
            for i in 1..face.len() - 1 {
                let b = self.points[face[i]];
                let c = self.points[face[i + 1]];
                let triangle_area = (b - anchor).cross(c - anchor).dot(normal) * 0.5;
                weighted_centre += (anchor + b + c) * (triangle_area / 3.0);
                signed_area += triangle_area;
            }
            let centre = weighted_centre / signed_area;
            face_geometry.push(FaceGeometry { area, centre });
        }

        let mut cell_points = vec![Vec::<usize>::new(); self.cell_count()];
        for (face_i, face) in self.faces.iter().enumerate() {
            let owner = self.owner[face_i];
            cell_points[owner].extend(face.iter().copied());
            if let Some(&neighbour) = self.neighbour.get(face_i) {
                cell_points[neighbour].extend(face.iter().copied());
            }
        }
        let mut cells = Vec::with_capacity(cell_points.len());
        for (cell_i, mut vertices) in cell_points.into_iter().enumerate() {
            vertices.sort_unstable();
            vertices.dedup();
            if vertices.is_empty() {
                return Err(MeshReadError::Invalid {
                    path: PathBuf::from("constant/polyMesh"),
                    detail: format!("cell {cell_i} has no vertices"),
                });
            }
            let reference = vertices
                .iter()
                .map(|&i| self.points[i])
                .fold(Vector3::zero(), |sum, p| sum + p)
                / vertices.len() as f64;
            let mut volume = 0.0;
            let mut first_moment = Vector3::zero();
            for (face_i, _) in self.faces.iter().enumerate() {
                let orientation = if self.owner[face_i] == cell_i {
                    1.0
                } else if self.neighbour.get(face_i) == Some(&cell_i) {
                    -1.0
                } else {
                    continue;
                };
                let fg = face_geometry[face_i];
                let pyramid_volume = orientation * fg.area.dot(fg.centre - reference) / 3.0;
                volume += pyramid_volume;
                first_moment += (reference + fg.centre * 3.0) * (pyramid_volume / 4.0);
            }
            if !volume.is_finite() || volume <= 0.0 {
                return Err(MeshReadError::Invalid {
                    path: PathBuf::from("constant/polyMesh"),
                    detail: format!("cell {cell_i} has non-positive volume {volume}"),
                });
            }
            cells.push(CellGeometry {
                centre: first_moment / volume,
                volume,
            });
        }
        Ok(MeshGeometry {
            faces: face_geometry,
            cells,
        })
    }

    fn validate(&self, mesh_dir: &Path) -> Result<(), MeshReadError> {
        let invalid = |detail: String| MeshReadError::Invalid {
            path: mesh_dir.to_path_buf(),
            detail,
        };
        if self.owner.len() != self.faces.len() {
            return Err(invalid(format!(
                "owner has {} entries but faces has {}",
                self.owner.len(),
                self.faces.len()
            )));
        }
        if self.neighbour.len() > self.faces.len() {
            return Err(invalid("neighbour has more entries than faces".into()));
        }
        for (face_i, face) in self.faces.iter().enumerate() {
            if face.len() < 3 {
                return Err(invalid(format!("face {face_i} has fewer than 3 vertices")));
            }
            for &point_i in face {
                if point_i >= self.points.len() {
                    return Err(invalid(format!(
                        "face {face_i} references missing point {point_i}"
                    )));
                }
            }
            if self.owner[face_i] >= self.cell_count() {
                return Err(invalid(format!("face {face_i} has invalid owner cell")));
            }
            if face_i < self.neighbour.len() && self.neighbour[face_i] >= self.cell_count() {
                return Err(invalid(format!("face {face_i} has invalid neighbour cell")));
            }
        }
        let mut ranges: Vec<(usize, usize, &str)> = self
            .patches
            .iter()
            .map(|p| (p.start_face, p.start_face + p.face_count, p.name.as_str()))
            .collect();
        ranges.sort_by_key(|r| r.0);
        let boundary_start = self.neighbour.len();
        let mut next = boundary_start;
        for (start, end, name) in ranges {
            if start != next || end > self.faces.len() {
                return Err(invalid(format!(
                    "boundary patch '{name}' has invalid or non-contiguous face range {start}..{end}"
                )));
            }
            next = end;
        }
        if next != self.faces.len() {
            return Err(invalid(format!(
                "boundary patches cover faces {boundary_start}..{next}, expected through {}",
                self.faces.len()
            )));
        }
        Ok(())
    }
}

fn read_points(path: &Path) -> Result<Vec<Vector3>, MeshReadError> {
    let (count, tokens) = read_list_tokens(path)?;
    let mut cursor = 0;
    let mut points = Vec::new();
    while cursor < tokens.len() {
        expect(&tokens, &mut cursor, "(", path)?;
        let x = number(&tokens, &mut cursor, path)?;
        let y = number(&tokens, &mut cursor, path)?;
        let z = number(&tokens, &mut cursor, path)?;
        expect(&tokens, &mut cursor, ")", path)?;
        points.push(Vector3 { x, y, z });
    }
    if points.len() != count {
        return Err(invalid(
            path,
            format!("declares {count} points but contains {}", points.len()),
        ));
    }
    Ok(points)
}

fn read_faces(path: &Path) -> Result<Vec<Vec<usize>>, MeshReadError> {
    let (count, tokens) = read_list_tokens(path)?;
    let mut cursor = 0;
    let mut faces = Vec::new();
    while cursor < tokens.len() {
        let count = label(&tokens, &mut cursor, path)?;
        expect(&tokens, &mut cursor, "(", path)?;
        let mut face = Vec::with_capacity(count);
        for _ in 0..count {
            face.push(label(&tokens, &mut cursor, path)?);
        }
        expect(&tokens, &mut cursor, ")", path)?;
        faces.push(face);
    }
    if faces.len() != count {
        return Err(invalid(
            path,
            format!("declares {count} faces but contains {}", faces.len()),
        ));
    }
    Ok(faces)
}

fn read_labels(path: &Path) -> Result<Vec<usize>, MeshReadError> {
    let (_, tokens) = read_list_tokens(path)?;
    tokens
        .iter()
        .map(|t| {
            t.parse::<usize>().map_err(|_| MeshReadError::Invalid {
                path: path.to_path_buf(),
                detail: format!("expected non-negative label, found '{t}'"),
            })
        })
        .collect()
}

fn read_boundary(path: &Path) -> Result<Vec<BoundaryPatch>, MeshReadError> {
    let tokens = read_tokens(path)?;
    let count_i = tokens
        .iter()
        .position(|token| token.parse::<usize>().is_ok())
        .ok_or_else(|| invalid(path, "missing boundary patch count"))?;
    let count = tokens[count_i]
        .parse::<usize>()
        .map_err(|_| invalid(path, "invalid patch count"))?;
    let open_i = tokens[count_i + 1..]
        .iter()
        .position(|token| token == "(")
        .map(|i| count_i + 1 + i)
        .ok_or_else(|| invalid(path, "missing boundary list opening parenthesis"))?;
    let mut cursor = open_i + 1;
    let mut patches = Vec::new();
    while cursor < tokens.len() && tokens[cursor] != ")" {
        let name = tokens[cursor].clone();
        cursor += 1;
        if name == "(" || name == ")" || name == ";" {
            continue;
        }
        expect(&tokens, &mut cursor, "{", path)?;
        let mut patch_type = None;
        let mut start_face = None;
        let mut face_count = None;
        let mut depth = 1usize;
        while cursor < tokens.len() && depth > 0 {
            match tokens[cursor].as_str() {
                "{" => {
                    depth += 1;
                    cursor += 1;
                }
                "}" => {
                    depth -= 1;
                    cursor += 1;
                }
                "type" if depth == 1 => {
                    cursor += 1;
                    patch_type = tokens.get(cursor).cloned();
                    cursor += usize::from(cursor < tokens.len());
                }
                "startFace" if depth == 1 => {
                    cursor += 1;
                    start_face = Some(parse_usize_token(&tokens, cursor, path)?);
                    cursor += 1;
                }
                "nFaces" if depth == 1 => {
                    cursor += 1;
                    face_count = Some(parse_usize_token(&tokens, cursor, path)?);
                    cursor += 1;
                }
                _ => cursor += 1,
            }
        }
        if let (Some(patch_type), Some(start_face), Some(face_count)) =
            (patch_type, start_face, face_count)
        {
            patches.push(BoundaryPatch {
                name,
                patch_type,
                start_face,
                face_count,
            });
        } else {
            return Err(MeshReadError::Invalid {
                path: path.to_path_buf(),
                detail: format!("patch '{name}' is missing type, startFace, or nFaces"),
            });
        }
    }
    if patches.len() != count {
        return Err(invalid(
            path,
            format!("declares {count} patches but contains {}", patches.len()),
        ));
    }
    Ok(patches)
}

fn read_list_tokens(path: &Path) -> Result<(usize, Vec<String>), MeshReadError> {
    let tokens = read_tokens(path)?;
    let count_i = tokens
        .iter()
        .position(|token| token.parse::<usize>().is_ok())
        .ok_or_else(|| invalid(path, "missing list length"))?;
    let count = tokens[count_i]
        .parse::<usize>()
        .map_err(|_| invalid(path, "invalid list length"))?;
    let open_i = tokens[count_i + 1..]
        .iter()
        .position(|token| token == "(")
        .map(|i| count_i + 1 + i)
        .ok_or_else(|| invalid(path, "missing list opening parenthesis"))?;
    let mut depth = 0usize;
    let mut close_i = None;
    for (i, token) in tokens.iter().enumerate().skip(open_i) {
        if token == "(" {
            depth += 1;
        }
        if token == ")" {
            depth -= 1;
            if depth == 0 {
                close_i = Some(i);
                break;
            }
        }
    }
    let close_i = close_i.ok_or_else(|| invalid(path, "unclosed list"))?;
    let body = tokens[open_i + 1..close_i].to_vec();
    // For flat label lists the item count is direct; nested point/face lists
    // are validated by their individual parsers.
    if !body.iter().any(|t| t == "(") && body.len() != count {
        return Err(invalid(
            path,
            format!("declares {count} entries but contains {}", body.len()),
        ));
    }
    Ok((count, body))
}

fn read_tokens(path: &Path) -> Result<Vec<String>, MeshReadError> {
    let input = fs::read_to_string(path).map_err(|source| MeshReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let clean = strip_comments(&input);
    let mut tokens = Vec::new();
    let mut word = String::new();
    for c in clean.chars() {
        if c.is_whitespace() {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
        } else if "(){};".contains(c) {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
            tokens.push(c.to_string());
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    Ok(tokens)
}

fn strip_comments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '\n' {
                    output.push('\n');
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
            output.push(c);
        }
    }
    output
}

fn expect(
    tokens: &[String],
    cursor: &mut usize,
    expected: &str,
    path: &Path,
) -> Result<(), MeshReadError> {
    if tokens.get(*cursor).map(String::as_str) == Some(expected) {
        *cursor += 1;
        Ok(())
    } else {
        Err(invalid(
            path,
            format!("expected '{expected}' at token {}", *cursor),
        ))
    }
}

fn number(tokens: &[String], cursor: &mut usize, path: &Path) -> Result<f64, MeshReadError> {
    let token = tokens
        .get(*cursor)
        .ok_or_else(|| invalid(path, "unexpected end of file"))?;
    let value = token
        .parse::<f64>()
        .map_err(|_| invalid(path, format!("expected number, found '{token}'")))?;
    *cursor += 1;
    Ok(value)
}

fn label(tokens: &[String], cursor: &mut usize, path: &Path) -> Result<usize, MeshReadError> {
    let value = parse_usize_token(tokens, *cursor, path)?;
    *cursor += 1;
    Ok(value)
}

fn parse_usize_token(
    tokens: &[String],
    cursor: usize,
    path: &Path,
) -> Result<usize, MeshReadError> {
    let token = tokens
        .get(cursor)
        .ok_or_else(|| invalid(path, "unexpected end of file"))?;
    token.parse::<usize>().map_err(|_| {
        invalid(
            path,
            format!("expected non-negative label, found '{token}'"),
        )
    })
}

fn invalid(path: &Path, detail: impl Into<String>) -> MeshReadError {
    MeshReadError::Invalid {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}
