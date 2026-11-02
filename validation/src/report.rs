use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::compare::ComparisonResult;

/// Status of a single test or comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    Pass,
    Fail,
    Error,
    Skip,
}

impl std::fmt::Display for TestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestStatus::Pass => write!(f, "PASS"),
            TestStatus::Fail => write!(f, "FAIL"),
            TestStatus::Error => write!(f, "ERROR"),
            TestStatus::Skip => write!(f, "SKIP"),
        }
    }
}

/// A single comparison entry in the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonEntry {
    pub field_name: String,
    pub status: TestStatus,
    #[serde(flatten)]
    pub result: Option<ComparisonResult>,
    /// Error message if status is Error.
    #[serde(default)]
    pub error_message: Option<String>,
    /// Reason for skip if status is Skip.
    #[serde(default)]
    pub skip_reason: Option<String>,
}

/// Performance metrics (separate from correctness).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    #[serde(default)]
    pub wall_clock_s: Option<f64>,
    #[serde(default)]
    pub peak_host_ram_mb: Option<f64>,
    #[serde(default)]
    pub peak_gpu_mem_mb: Option<f64>,
    #[serde(default)]
    pub n_cells: Option<usize>,
    #[serde(default)]
    pub n_iterations: Option<usize>,
    #[serde(default)]
    pub iterations_per_second: Option<f64>,
    #[serde(default)]
    pub data_load_time_s: Option<f64>,
    #[serde(default)]
    pub precision: Option<String>,
}

/// Full validation report for one test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseReport {
    /// Test case identifier.
    pub case_name: String,
    /// Solver under test.
    pub solver: String,
    /// Solver version/revision.
    #[serde(default)]
    pub solver_version: Option<String>,
    /// Reference source description.
    pub reference_source: String,
    /// Timestamp of the validation run.
    pub timestamp: String,
    /// Individual field comparisons.
    pub comparisons: Vec<ComparisonEntry>,
    /// Overall status.
    pub overall_status: TestStatus,
    /// Performance metrics (optional).
    #[serde(default)]
    pub performance: Option<PerformanceMetrics>,
    /// Human-readable summary line.
    pub summary: String,
}

impl CaseReport {
    /// Determine overall status from individual comparisons.
    pub fn compute_overall(&mut self) {
        if self.comparisons.iter().any(|c| c.status == TestStatus::Error) {
            self.overall_status = TestStatus::Error;
        } else if self.comparisons.iter().any(|c| c.status == TestStatus::Fail) {
            self.overall_status = TestStatus::Fail;
        } else if self.comparisons.is_empty()
            || self.comparisons.iter().all(|c| c.status == TestStatus::Skip)
        {
            self.overall_status = TestStatus::Skip;
        } else {
            self.overall_status = TestStatus::Pass;
        }

        let n_pass = self.comparisons.iter().filter(|c| c.status == TestStatus::Pass).count();
        let n_total = self.comparisons.len();
        self.summary = format!(
            "{}: {}/{} fields passed",
            self.overall_status, n_pass, n_total
        );
    }
}

/// Aggregate report across multiple cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// All case reports.
    pub cases: Vec<CaseReport>,
    /// Overall pass/fail (true if no FAIL or ERROR).
    pub all_passed: bool,
    /// Generation timestamp.
    pub generated_at: String,
}

impl ValidationReport {
    pub fn new(cases: Vec<CaseReport>) -> Self {
        let all_passed = cases
            .iter()
            .all(|c| c.overall_status == TestStatus::Pass || c.overall_status == TestStatus::Skip);
        Self {
            all_passed,
            cases,
            generated_at: crate::provenance::chrono_now_public(),
        }
    }

    /// Write machine-readable JSON report.
    pub fn write_json(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)
    }

    /// Write human-readable summary.
    pub fn write_summary(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = String::new();
        out.push_str("# FoamCore Validation Report\n\n");
        out.push_str(&format!("Generated: {}\n\n", self.generated_at));
        out.push_str(&format!(
            "Overall: {}\n\n",
            if self.all_passed { "ALL PASSED" } else { "FAILURES DETECTED" }
        ));

        for case in &self.cases {
            out.push_str(&format!("## {} — {}\n\n", case.case_name, case.summary));
            for entry in &case.comparisons {
                match &entry.result {
                    Some(r) => {
                        out.push_str(&format!("  - {}: {}\n", entry.field_name, r));
                    }
                    None => {
                        out.push_str(&format!(
                            "  - {}: {} — {}\n",
                            entry.field_name,
                            entry.status,
                            entry.error_message.as_deref().or(entry.skip_reason.as_deref()).unwrap_or("")
                        ));
                    }
                }
            }
            out.push('\n');
        }

        std::fs::write(path, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{ComparisonResult, Tolerance};
    use crate::field::{Field, FieldType};

    fn make_pass_result(name: &str) -> ComparisonResult {
        ComparisonResult {
            field_name: name.to_string(),
            field_type: FieldType::Scalar,
            n_values: 10,
            max_abs_error: 1e-15,
            rmse: 5e-16,
            rel_l2_error: 1e-14,
            max_component_errors: None,
            has_non_finite: false,
            first_violation_index: None,
            first_violation_values: None,
            passed: true,
            tolerance: Tolerance::default(),
        }
    }

    #[test]
    fn test_report_all_pass() {
        let mut report = CaseReport {
            case_name: "test".into(),
            solver: "foamcore".into(),
            solver_version: Some("0.1.0".into()),
            reference_source: "analytical".into(),
            timestamp: "now".into(),
            comparisons: vec![
                ComparisonEntry {
                    field_name: "U".into(),
                    status: TestStatus::Pass,
                    result: Some(make_pass_result("U")),
                    error_message: None,
                    skip_reason: None,
                },
                ComparisonEntry {
                    field_name: "phi".into(),
                    status: TestStatus::Pass,
                    result: Some(make_pass_result("phi")),
                    error_message: None,
                    skip_reason: None,
                },
            ],
            overall_status: TestStatus::Skip, // will be recomputed
            performance: None,
            summary: String::new(),
        };
        report.compute_overall();
        assert_eq!(report.overall_status, TestStatus::Pass);
    }

    #[test]
    fn test_report_with_error() {
        let mut report = CaseReport {
            case_name: "test".into(),
            solver: "foamcore".into(),
            solver_version: None,
            reference_source: "openfoam".into(),
            timestamp: "now".into(),
            comparisons: vec![ComparisonEntry {
                field_name: "U".into(),
                status: TestStatus::Error,
                result: None,
                error_message: Some("mesh mismatch".into()),
                skip_reason: None,
            }],
            overall_status: TestStatus::Skip,
            performance: None,
            summary: String::new(),
        };
        report.compute_overall();
        assert_eq!(report.overall_status, TestStatus::Error);
    }

    #[test]
    fn test_validation_report_json_roundtrip() {
        let case = CaseReport {
            case_name: "simpleChannel".into(),
            solver: "foamcore".into(),
            solver_version: Some("0.1.0".into()),
            reference_source: "analytical".into(),
            timestamp: "2024-01-01T00:00:00Z".into(),
            comparisons: vec![],
            overall_status: TestStatus::Pass,
            performance: None,
            summary: "PASS: 0/0 fields passed".into(),
        };
        let vr = ValidationReport::new(vec![case]);
        let json = serde_json::to_string(&vr).unwrap();
        let parsed: ValidationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cases.len(), 1);
        assert_eq!(parsed.cases[0].case_name, "simpleChannel");
    }
}
