use serde::{Deserialize, Serialize};
use std::path::Path;

/// Provenance metadata for a reference dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// Reference format version (schema compatibility).
    pub format_version: u32,
    /// Solver name.
    pub solver: String,
    /// Distribution (e.g., "OpenFOAM Foundation").
    pub distribution: String,
    /// Version string.
    pub version: String,
    /// Git commit hash (empty if unavailable).
    pub commit: String,
    /// Whether the source tree had local modifications.
    pub dirty_tree: bool,
    /// Floating-point precision used in the build.
    pub precision: String,
    /// Platform/build identifier.
    pub platform: String,
    /// Compiler identification (if discoverable).
    #[serde(default)]
    pub compiler: Option<String>,
    /// Case name.
    pub case_name: String,
    /// SHA-256 of the input case directory (all files concatenated in sorted order).
    #[serde(default)]
    pub input_checksum: Option<String>,
    /// SHA-256 of the mesh (polyMesh) directory.
    #[serde(default)]
    pub mesh_checksum: Option<String>,
    /// ISO 8601 timestamp of when the reference was generated.
    pub generated_at: String,
    /// Wall-clock duration of the OpenFOAM run in seconds.
    #[serde(default)]
    pub execution_time_s: Option<f64>,
}

impl Provenance {
    /// Create a new provenance record with current timestamp.
    pub fn new(solver: &str, distribution: &str, version: &str, case_name: &str) -> Self {
        Self {
            format_version: 1,
            solver: solver.to_string(),
            distribution: distribution.to_string(),
            version: version.to_string(),
            commit: String::new(),
            dirty_tree: false,
            precision: "float64".to_string(),
            platform: String::new(),
            compiler: None,
            case_name: case_name.to_string(),
            input_checksum: None,
            mesh_checksum: None,
            generated_at: chrono_now(),
            execution_time_s: None,
        }
    }

    /// Write provenance to a TOML file.
    pub fn write(&self, path: &Path) -> Result<(), std::io::Error> {
        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, toml_str)
    }

    /// Read provenance from a TOML file.
    pub fn read(path: &Path) -> Result<Self, ProvenanceError> {
        let content = std::fs::read_to_string(path).map_err(ProvenanceError::Io)?;
        toml::from_str(&content).map_err(ProvenanceError::Parse)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProvenanceError {
    #[error("IO error reading provenance: {0}")]
    Io(#[source] std::io::Error),
    #[error("Failed to parse provenance TOML: {0}")]
    Parse(#[source] toml::de::Error),
}

/// Compute SHA-256 checksum of all files in a directory (sorted by relative path).
pub fn directory_checksum(dir: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};

    let mut files: Vec<std::path::PathBuf> = collect_files(dir);
    files.sort();

    if files.is_empty() {
        return None;
    }

    let mut hasher = Sha256::new();
    for file in &files {
        let rel = file.strip_prefix(dir).unwrap_or(file);
        // Include filename in hash for identity
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        if let Ok(content) = std::fs::read(file) {
            hasher.update(&content);
        }
    }

    Some(hex::encode(hasher.finalize()))
}

fn collect_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                out.push(path);
            } else if path.is_dir() {
                out.extend(collect_files(&path));
            }
        }
    }
    out
}

/// Get current UTC time as ISO 8601 string (public for use by report module).
pub fn chrono_now_public() -> String {
    chrono_now()
}

/// Get current UTC time as ISO 8601 string.
fn chrono_now() -> String {
    // Simple implementation without external chrono dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Convert to date-time (simplified — good enough for provenance timestamps)
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Days since epoch to Y-M-D (civil from days algorithm)
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since 1970-01-01 to (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provenance_roundtrip() {
        let p = Provenance::new("potentialFoam", "OpenFOAM Foundation", "14", "simpleChannel");
        let toml_str = toml::to_string_pretty(&p).unwrap();
        let parsed: Provenance = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.solver, "potentialFoam");
        assert_eq!(parsed.case_name, "simpleChannel");
        assert_eq!(parsed.format_version, 1);
    }

    #[test]
    fn test_directory_checksum_deterministic() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../corpus/simpleChannel"));
        if dir.exists() {
            let h1 = directory_checksum(dir);
            let h2 = directory_checksum(dir);
            assert_eq!(h1, h2);
            assert!(h1.is_some());
        }
    }

    #[test]
    fn test_civil_from_days_known_date() {
        // 2024-01-15 is day 19737 since epoch
        let (y, m, d) = civil_from_days(19737);
        assert_eq!((y, m, d), (2024, 1, 15));
    }
}
