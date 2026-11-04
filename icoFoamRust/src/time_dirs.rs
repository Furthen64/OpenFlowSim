//! Time directory management for OpenFOAM cases.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TimeDirError {
    #[error("failed to create {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("time directory '{0}' does not exist")]
    NotFound(PathBuf),
}

/// Helpers for reading/writing fields in OpenFOAM time directories.
pub struct TimeDirs {
    case_dir: PathBuf,
}

impl TimeDirs {
    pub fn new(case_dir: &Path) -> Self {
        Self { case_dir: case_dir.to_path_buf() }
    }

    /// Path to the initial (zero) time directory where BCs are defined.
    pub fn zero(&self) -> PathBuf {
        self.case_dir.join("0")
    }

    /// Path to a specific time directory.
    pub fn at(&self, t: f64) -> PathBuf {
        self.case_dir.join(format_time(t))
    }

    /// Ensure a time directory exists, creating it if necessary.
    pub fn ensure(&self, t: f64) -> Result<PathBuf, TimeDirError> {
        let dir = self.at(t);
        if !dir.is_dir() {
            fs::create_dir_all(&dir).map_err(|source| TimeDirError::Io {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(dir)
    }

    /// Read a field from the latest available time directory <= `t`.
    /// Falls back to `0/` for the initial condition.
    pub fn read_field_path(&self, name: &str, t: f64) -> PathBuf {
        // Check exact time first, then fall back to 0/
        let exact = self.at(t).join(name);
        if exact.exists() {
            return exact;
        }
        self.zero().join(name)
    }

    /// Write a field into a time directory (creating the dir if needed).
    pub fn write_field_path(&self, name: &str, t: f64) -> Result<PathBuf, TimeDirError> {
        let dir = self.ensure(t)?;
        Ok(dir.join(name))
    }
}

/// Format a time value as OpenFOAM does (trim trailing zeros).
pub fn format_time(t: f64) -> String {
    if t == t.trunc() && t.abs() < 1e15 {
        format!("{}", t as i64)
    } else {
        let s = format!("{t:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_integer() {
        assert_eq!(format_time(0.0), "0");
        assert_eq!(format_time(1.0), "1");
        assert_eq!(format_time(100.0), "100");
    }

    #[test]
    fn format_time_fractional() {
        assert_eq!(format_time(0.5), "0.5");
        assert_eq!(format_time(0.01), "0.01");
        assert_eq!(format_time(1.25), "1.25");
    }

    #[test]
    fn zero_dir_path() {
        let td = TimeDirs::new(Path::new("/tmp/case"));
        assert_eq!(td.zero(), PathBuf::from("/tmp/case/0"));
    }

    #[test]
    fn at_dir_path() {
        let td = TimeDirs::new(Path::new("/tmp/case"));
        assert_eq!(td.at(0.5), PathBuf::from("/tmp/case/0.5"));
        assert_eq!(td.at(1.0), PathBuf::from("/tmp/case/1"));
    }
}
