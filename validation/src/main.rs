use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use foamcore_validation::compare::{compare_fields, compare_fields_rel_l2, gauge_align, Tolerance};
use foamcore_validation::field::{self, Field, FieldType};
use foamcore_validation::provenance::{directory_checksum, Provenance};
use foamcore_validation::report::{CaseReport, ComparisonEntry, TestStatus, ValidationReport};

#[derive(Parser)]
#[command(name = "foamcore-validate")]
#[command(about = "Numerical verification harness for FoamCore CFD solver")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run numerical comparison between results/ and reference/ for a case
    Compare {
        /// Case name (e.g., simpleChannel, cylinder)
        case: String,
        /// Path to tolerances file
        #[arg(long, default_value = "validation/tolerances.toml")]
        tolerances: PathBuf,
        /// Results directory
        #[arg(long, default_value = "results")]
        results_dir: PathBuf,
        /// Reference directory
        #[arg(long, default_value = "reference")]
        ref_dir: PathBuf,
        /// Output report path
        #[arg(long, default_value = "validation/reports/report.json")]
        report: PathBuf,
        /// Directory for generated SVG field plots
        #[arg(long)]
        plots_dir: Option<PathBuf>,
        /// OpenFOAM case containing constant/polyMesh (required with --plots-dir)
        #[arg(long)]
        mesh_case: Option<PathBuf>,
    },
    /// Run all available comparison cases
    CompareAll {
        #[arg(long, default_value = "validation/tolerances.toml")]
        tolerances: PathBuf,
        #[arg(long, default_value = "results")]
        results_dir: PathBuf,
        #[arg(long, default_value = "reference")]
        ref_dir: PathBuf,
        #[arg(long, default_value = "validation/reports/report_all.json")]
        report: PathBuf,
    },
    /// Run analytical verification (no OpenFOAM needed)
    Analytical {
        /// Case name
        #[arg(default_value = "simpleChannel")]
        case: String,
        /// Results directory containing solver output
        #[arg(long, default_value = "results")]
        results_dir: PathBuf,
        #[arg(long, default_value = "validation/reports/analytical.json")]
        report: PathBuf,
    },
    /// Check backward-facing-step separation and streamwise profiles
    StepDiagnostics {
        /// Results directory containing backwardFacingStep/<time>/U
        #[arg(long, default_value = "results")]
        results_dir: PathBuf,
        /// Reference directory containing backwardFacingStep/<time>/U
        #[arg(long, default_value = "reference")]
        ref_dir: PathBuf,
        /// Case directory containing constant/polyMesh
        #[arg(long)]
        mesh_case: PathBuf,
        /// CSV output for the sampled station profiles
        #[arg(
            long,
            default_value = "validation/reports/backwardFacingStep_profiles.csv"
        )]
        output: PathBuf,
    },
    /// Generate reference data using OpenFOAM (requires OpenFOAM installation)
    GenerateReference {
        /// Case name
        case: String,
        /// Path to corpus directory
        #[arg(long, default_value = "corpus")]
        corpus_dir: PathBuf,
        /// Output reference directory
        #[arg(long, default_value = "reference")]
        ref_dir: PathBuf,
        /// Force overwrite existing reference (dangerous!)
        #[arg(long)]
        force: bool,
        /// Path to bashof.sh or OpenFOAM env setup script
        #[arg(long)]
        openfoam_env: Option<PathBuf>,
    },
    /// Validate provenance metadata of a reference dataset
    CheckProvenance {
        /// Reference directory
        #[arg(long, default_value = "reference")]
        ref_dir: PathBuf,
        /// Case name
        case: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Compare {
            case,
            tolerances,
            results_dir,
            ref_dir,
            report,
            plots_dir,
            mesh_case,
        } => cmd_compare(
            &case,
            &tolerances,
            &results_dir,
            &ref_dir,
            &report,
            plots_dir.as_deref(),
            mesh_case.as_deref(),
        ),

        Commands::CompareAll {
            tolerances,
            results_dir,
            ref_dir,
            report,
        } => cmd_compare_all(&tolerances, &results_dir, &ref_dir, &report),

        Commands::Analytical {
            case,
            results_dir,
            report,
        } => cmd_analytical(&case, &results_dir, &report),

        Commands::StepDiagnostics {
            results_dir,
            ref_dir,
            mesh_case,
            output,
        } => cmd_step_diagnostics(&results_dir, &ref_dir, &mesh_case, &output),

        Commands::GenerateReference {
            case,
            corpus_dir,
            ref_dir,
            force,
            openfoam_env,
        } => cmd_generate_reference(&case, &corpus_dir, &ref_dir, force, openfoam_env.as_deref()),

        Commands::CheckProvenance { ref_dir, case } => cmd_check_provenance(&ref_dir, &case),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::from(2)
        }
    }
}

fn load_tolerances(path: &Path) -> Result<Tolerance, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(Tolerance::default());
    }
    let content = std::fs::read_to_string(path)?;
    let val: toml::Value = toml::from_str(&content)?;
    let default = val.get("default");
    let atol = default
        .and_then(|d| d.get("atol"))
        .and_then(|v| v.as_float())
        .unwrap_or(1e-10);
    let rtol = default
        .and_then(|d| d.get("rtol"))
        .and_then(|v| v.as_float())
        .unwrap_or(1e-8);
    Ok(Tolerance { atol, rtol })
}

fn get_field_tolerance(toml_path: &Path, case: &str, field_name: &str) -> Tolerance {
    // Try case-specific, then field-specific, then default
    if toml_path.exists() {
        if let Ok(content) = std::fs::read_to_string(toml_path) {
            if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                // cases.<case>.fields.<field>
                if let Some(c) = val
                    .get("cases")
                    .and_then(|c| c.get(case))
                    .and_then(|c| c.get("fields"))
                    .and_then(|f| f.get(field_name))
                {
                    return Tolerance {
                        atol: c.get("atol").and_then(|v| v.as_float()).unwrap_or(1e-10),
                        rtol: c.get("rtol").and_then(|v| v.as_float()).unwrap_or(1e-8),
                    };
                }
                // fields.<field>
                if let Some(f) = val.get("fields").and_then(|f| f.get(field_name)) {
                    return Tolerance {
                        atol: f.get("atol").and_then(|v| v.as_float()).unwrap_or(1e-10),
                        rtol: f.get("rtol").and_then(|v| v.as_float()).unwrap_or(1e-8),
                    };
                }
            }
        }
    }
    load_tolerances(toml_path).unwrap_or_default()
}

fn cmd_compare(
    case: &str,
    tol_path: &Path,
    results_dir: &Path,
    ref_dir: &Path,
    report_path: &Path,
    plots_dir: Option<&Path>,
    mesh_case: Option<&Path>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let case_ref = ref_dir.join(case);
    let case_results = results_dir.join(case);

    if !case_ref.exists() {
        eprintln!(
            "ERROR: Reference directory not found: {}",
            case_ref.display()
        );
        eprintln!("  Generate it with: foamcore-validate generate-reference --case {case}");
        return Ok(ExitCode::from(2));
    }

    if !case_results.exists() {
        eprintln!(
            "ERROR: Results directory not found: {}",
            case_results.display()
        );
        eprintln!("  Run the solver first to produce output.");
        return Ok(ExitCode::from(2));
    }

    if let Some(plots_dir) = plots_dir {
        let mesh_case = mesh_case.ok_or("--mesh-case is required when --plots-dir is used")?;
        write_case_plots(&case_results, &case_ref, mesh_case, plots_dir, case)?;
    }

    // Load provenance for reporting
    let prov_path = case_ref.join("provenance.toml");
    let reference_source = if prov_path.exists() {
        format!("OpenFOAM (see {})", prov_path.display())
    } else {
        "unknown".to_string()
    };

    let mut comparisons = Vec::new();

    // Compare U field
    comparisons.push(compare_field_file(
        "U",
        &case_results,
        &case_ref,
        tol_path,
        case,
        FieldType::Vector,
    ));

    // Pressure is defined up to a constant unless both cases use the same
    // reference cell, so align its gauge before comparing.
    comparisons.push(compare_field_file(
        "p",
        &case_results,
        &case_ref,
        tol_path,
        case,
        FieldType::Scalar,
    ));

    // Compare phi field
    comparisons.push(compare_field_file(
        "phi",
        &case_results,
        &case_ref,
        tol_path,
        case,
        FieldType::Scalar,
    ));

    // Compare Phi field (if present in both)
    let phi_result_path = case_results.join("1").join("Phi");
    let phi_ref_path = case_ref.join("Phi");
    if phi_result_path.exists() && phi_ref_path.exists() {
        comparisons.push(compare_field_file(
            "Phi",
            &case_results,
            &case_ref,
            tol_path,
            case,
            FieldType::Scalar,
        ));
    }

    let mut report = CaseReport {
        case_name: case.to_string(),
        solver: "foamcore-rust".into(),
        solver_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        reference_source,
        timestamp: foamcore_validation::provenance::chrono_now_public(),
        comparisons,
        overall_status: TestStatus::Skip,
        performance: None,
        summary: String::new(),
    };
    report.compute_overall();

    // Write reports
    let vr = ValidationReport::new(vec![report.clone()]);
    vr.write_json(report_path)?;
    let summary_path = report_path.with_extension("md");
    vr.write_summary(&summary_path)?;

    println!("{}", report.summary);
    for entry in &report.comparisons {
        match &entry.result {
            Some(r) => println!("  {r}"),
            None => println!(
                "  {}: {} — {}",
                entry.field_name,
                entry.status,
                entry.error_message.as_deref().unwrap_or("")
            ),
        }
    }
    println!("\nReport written to: {}", report_path.display());

    if report.overall_status == TestStatus::Pass || report.overall_status == TestStatus::Skip {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn compare_field_file(
    name: &str,
    results_dir: &Path,
    ref_dir: &Path,
    tol_path: &Path,
    case: &str,
    ftype: FieldType,
) -> ComparisonEntry {
    let result_path = find_field_path(results_dir, name);
    let ref_path = find_field_path(ref_dir, name);

    let (result_field, ref_field) = match (result_path, ref_path) {
        (Some(rp), Some(fp)) => {
            let rf = parse_field_file(&rp, name, ftype);
            let ff = parse_field_file(&fp, name, ftype);
            match (rf, ff) {
                (Ok(r), Ok(f)) => (r, f),
                (Err(e), _) => {
                    return ComparisonEntry {
                        field_name: name.to_string(),
                        status: TestStatus::Error,
                        result: None,
                        error_message: Some(format!("Failed to parse results: {e}")),
                        skip_reason: None,
                    }
                }
                (_, Err(e)) => {
                    return ComparisonEntry {
                        field_name: name.to_string(),
                        status: TestStatus::Error,
                        result: None,
                        error_message: Some(format!("Failed to parse reference: {e}")),
                        skip_reason: None,
                    }
                }
            }
        }
        (None, _) => {
            return ComparisonEntry {
                field_name: name.to_string(),
                status: TestStatus::Error,
                result: None,
                error_message: Some("Results file not found".into()),
                skip_reason: None,
            }
        }
        (_, None) => {
            return ComparisonEntry {
                field_name: name.to_string(),
                status: TestStatus::Error,
                result: None,
                error_message: Some("Reference file not found".into()),
                skip_reason: None,
            }
        }
    };

    let tol = get_field_tolerance(tol_path, case, name);

    // Gauge align Phi if configured
    let mut rf = result_field;
    if name == "Phi" || name == "p" {
        gauge_align(&mut rf, &ref_field);
    }

    let comparison = relative_l2_target(tol_path, case, name).map_or_else(
        || compare_fields(&rf, &ref_field, &tol),
        |target| compare_fields_rel_l2(&rf, &ref_field, target),
    );
    match comparison {
        Ok(cr) => ComparisonEntry {
            field_name: name.to_string(),
            status: if cr.passed {
                TestStatus::Pass
            } else {
                TestStatus::Fail
            },
            result: Some(cr),
            error_message: None,
            skip_reason: None,
        },
        Err(e) => ComparisonEntry {
            field_name: name.to_string(),
            status: TestStatus::Error,
            result: None,
            error_message: Some(format!("{e}")),
            skip_reason: None,
        },
    }
}

fn relative_l2_target(toml_path: &Path, case: &str, field_name: &str) -> Option<f64> {
    let content = std::fs::read_to_string(toml_path).ok()?;
    let value: toml::Value = toml::from_str(&content).ok()?;
    value
        .get("cases")?
        .get(case)?
        .get("fields")?
        .get(field_name)?
        .get("relative_l2")?
        .as_float()
}

fn find_field_path(root: &Path, name: &str) -> Option<PathBuf> {
    let mut times: Vec<(f64, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let time = entry.file_name().to_string_lossy().parse::<f64>().ok()?;
            let path = entry.path().join(name);
            path.exists().then_some((time, path))
        })
        .collect();
    times.sort_by(|a, b| a.0.total_cmp(&b.0));
    times.pop().map(|(_, path)| path).or_else(|| {
        [
            root.join("1").join(name),
            root.join("0").join(name),
            root.join(name),
        ]
        .into_iter()
        .find(|p| p.exists())
    })
}

fn write_case_plots(
    results_case: &Path,
    reference_case: &Path,
    mesh_case: &Path,
    output_dir: &Path,
    case: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use foamcore_solver::mesh::PolyMesh;

    let mesh = PolyMesh::read_case(mesh_case)?;
    let geometry = mesh.geometry()?;
    std::fs::create_dir_all(output_dir)?;
    for (name, ftype) in [("U", FieldType::Vector), ("p", FieldType::Scalar)] {
        let result_path = find_field_path(results_case, name)
            .ok_or_else(|| format!("missing result field {name} for plot"))?;
        let reference_path = find_field_path(reference_case, name)
            .ok_or_else(|| format!("missing reference field {name} for plot"))?;
        let mut result = parse_field_file(&result_path, name, ftype)?;
        let reference = parse_field_file(&reference_path, name, ftype)?;
        if name == "p" {
            gauge_align(&mut result, &reference);
        }
        let path = output_dir.join(format!("{case}_{name}.svg"));
        foamcore_validation::plot::write_field_plot(&path, &reference, &result, &geometry)
            .map_err(|e| format!("failed to plot {name}: {e}"))?;
        println!("Field plot written to: {}", path.display());
    }
    if case == "cavity" {
        let result_path = find_field_path(results_case, "U").ok_or("U field not found")?;
        let reference_path =
            find_field_path(reference_case, "U").ok_or("reference U field not found")?;
        let result_u = parse_field_file(&result_path, "U", FieldType::Vector)?;
        let reference_u = parse_field_file(&reference_path, "U", FieldType::Vector)?;
        let report = foamcore_validation::analytical::cavity_flow_diagnostics(
            &result_u,
            &reference_u,
            &geometry,
        )
        .map_err(|e| format!("failed to analyze cavity streamfunction: {e}"))?;
        let result_psi =
            foamcore_validation::analytical::cavity_streamfunction(&result_u, &geometry)
                .map_err(|e| format!("failed to reconstruct Rust streamfunction: {e}"))?;
        let reference_psi =
            foamcore_validation::analytical::cavity_streamfunction(&reference_u, &geometry)
                .map_err(|e| format!("failed to reconstruct reference streamfunction: {e}"))?;
        let path = output_dir.join("cavity_streamfunction.svg");
        foamcore_validation::plot::write_field_plot(&path, &reference_psi, &result_psi, &geometry)
            .map_err(|e| format!("failed to plot cavity streamfunction: {e}"))?;
        println!(
            "Cavity psi rel_l2={:.3e}; primary vortex centres reference=({:.4},{:.4}) Rust=({:.4},{:.4}) distance={:.3e}; corner extrema reference={} Rust={}",
            report.streamfunction_relative_l2,
            report.primary_reference.x,
            report.primary_reference.y,
            report.primary_computed.x,
            report.primary_computed.y,
            report.primary_center_distance,
            report.reference_corner_extrema,
            report.computed_corner_extrema
        );
        println!("Field plot written to: {}", path.display());
    }
    Ok(())
}

fn parse_field_file(path: &Path, name: &str, ftype: FieldType) -> Result<Field, String> {
    match ftype {
        FieldType::Scalar => {
            let values = field::parse_openfoam_scalar_internal(path).map_err(|e| e.to_string())?;
            Ok(Field::scalar(name, values))
        }
        FieldType::Vector => {
            let (x, y, z) =
                field::parse_openfoam_vector_internal(path).map_err(|e| e.to_string())?;
            Ok(Field::vector(name, x, y, z))
        }
    }
}

fn cmd_compare_all(
    tol_path: &Path,
    results_dir: &Path,
    ref_dir: &Path,
    report_path: &Path,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    // Discover cases from reference directory
    let mut cases = Vec::new();
    if ref_dir.exists() {
        for entry in std::fs::read_dir(ref_dir)?.flatten() {
            if entry.path().is_dir() {
                cases.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    if cases.is_empty() {
        eprintln!("No reference cases found in {}", ref_dir.display());
        return Ok(ExitCode::from(2));
    }

    println!(
        "Running comparisons for {} case(s): {}\n",
        cases.len(),
        cases.join(", ")
    );

    let mut reports = Vec::new();
    let mut any_failed = false;

    for case in &cases {
        print!("  {}: ", case);
        let code = cmd_compare(
            case,
            tol_path,
            results_dir,
            ref_dir,
            &report_path.with_file_name(format!("{case}.json")),
            None,
            None,
        );
        match code {
            Ok(c) => {
                if c == ExitCode::SUCCESS {
                    println!("OK");
                } else {
                    println!("FAILED");
                    any_failed = true;
                }
            }
            Err(e) => {
                println!("ERROR: {e}");
                any_failed = true;
            }
        }
        // We don't aggregate into a single report here — each case writes its own.
        // For the aggregate, we'd need to restructure. Keep it simple for now.
        reports.push(CaseReport {
            case_name: case.clone(),
            solver: "foamcore-rust".into(),
            solver_version: None,
            reference_source: "see individual reports".into(),
            timestamp: String::new(),
            comparisons: vec![],
            overall_status: if any_failed {
                TestStatus::Fail
            } else {
                TestStatus::Pass
            },
            performance: None,
            summary: String::new(),
        });
    }

    let _ = reports; // Individual reports already written
    println!("\nAll comparisons complete.");

    if any_failed {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn cmd_analytical(
    case: &str,
    results_dir: &Path,
    report_path: &Path,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    use foamcore_validation::analytical::UniformChannelAnalytical;

    if case == "pipeFlow" {
        let case_results = results_dir.join(case);
        let u_path = find_field_path(&case_results, "U").ok_or("U field not found in results")?;
        let (ux, uy, uz) = field::parse_openfoam_vector_internal(&u_path)
            .map_err(|e| format!("Failed to parse U: {e}"))?;
        let computed_u = Field::vector("U", ux, uy, uz);
        let mesh =
            foamcore_solver::mesh::PolyMesh::read_case(Path::new("icoFoamRust/cases/pipeFlow"))?;
        let geometry = mesh.geometry()?;
        let result = foamcore_validation::analytical::verify_poiseuille_outlet(
            &computed_u,
            &geometry,
            1.0,
            1.0,
            1.5,
            0.02,
        )?;
        println!("{result}");
        if let Some(parent) = report_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(report_path, serde_json::to_string_pretty(&result)?)?;
        println!("\nReport: {}", report_path.display());
        return Ok(if result.passed {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }

    if case != "simpleChannel" {
        eprintln!("ERROR: Analytical verification supports 'simpleChannel' and 'pipeFlow'");
        return Ok(ExitCode::from(2));
    }

    // Load case metadata to get mesh parameters
    let case_toml = Path::new("corpus").join(case).join("case.toml");
    let (nx, ny) = if case_toml.exists() {
        let content = std::fs::read_to_string(&case_toml)?;
        let val: toml::Value = toml::from_str(&content)?;
        let cpa = val
            .get("geometry")
            .and_then(|g| g.get("cells_per_axis"))
            .and_then(|c| c.as_array());
        if let Some(arr) = cpa {
            (
                arr[0].as_integer().unwrap_or(40) as usize,
                arr[1].as_integer().unwrap_or(10) as usize,
            )
        } else {
            (40, 10)
        }
    } else {
        (40, 10)
    };

    let u0 = 1.0_f64;
    let length = 2.0_f64;
    let analytical = UniformChannelAnalytical::new(u0, length, nx, ny);

    // Load computed results
    let case_results = results_dir.join(case);
    if !case_results.exists() {
        eprintln!(
            "ERROR: No solver results found at {}",
            case_results.display()
        );
        eprintln!("  Run the Rust solver first.");
        return Ok(ExitCode::from(2));
    }

    let u_candidates = vec![
        case_results.join("1").join("U"),
        case_results.join("0").join("U"),
        case_results.join("U"),
    ];
    let u_path = u_candidates
        .iter()
        .find(|p| p.exists())
        .ok_or("U field not found in results")?;

    let (ux, uy, uz) = field::parse_openfoam_vector_internal(u_path)
        .map_err(|e| format!("Failed to parse U: {e}"))?;
    let computed_u = Field::vector("U", ux, uy, uz);

    // Phi is optional
    let phi_candidates = vec![
        case_results.join("1").join("Phi"),
        case_results.join("0").join("Phi"),
        case_results.join("Phi"),
    ];
    let phi_path = phi_candidates.iter().find(|p| p.exists());
    let computed_phi = if let Some(pp) = phi_path {
        let vals = field::parse_openfoam_scalar_internal(pp)
            .map_err(|e| format!("Failed to parse Phi: {e}"))?;
        Some(Field::scalar("Phi", vals))
    } else {
        None
    };

    // The analytical case is also run against OpenFOAM output, whose iterative
    // solve leaves residuals around 1e-7 for this mesh and solver tolerance.
    let tol_u = Tolerance {
        atol: 1e-6,
        rtol: 1e-6,
    };
    let tol_p = Tolerance {
        atol: 1e-6,
        rtol: 1e-6,
    };

    let report = analytical.verify(&computed_u, computed_phi.as_ref(), &tol_u, &tol_p);
    println!("{report}");

    // Write JSON report
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(report_path, json)?;
    println!("\nReport: {}", report_path.display());

    if report.all_passed {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn cmd_step_diagnostics(
    results_dir: &Path,
    ref_dir: &Path,
    mesh_case: &Path,
    output: &Path,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    use foamcore_solver::mesh::PolyMesh;
    use foamcore_validation::analytical::backward_facing_step_diagnostics;

    let case_results = results_dir.join("backwardFacingStep");
    let case_reference = ref_dir.join("backwardFacingStep");
    let result_path = find_field_path(&case_results, "U").ok_or("result U field not found")?;
    let reference_path =
        find_field_path(&case_reference, "U").ok_or("reference U field not found")?;
    let computed_u = parse_field_file(&result_path, "U", FieldType::Vector)?;
    let reference_u = parse_field_file(&reference_path, "U", FieldType::Vector)?;
    let mesh = PolyMesh::read_case(mesh_case)?;
    let geometry = mesh.geometry()?;
    let report = backward_facing_step_diagnostics(
        &computed_u,
        &reference_u,
        &geometry,
        0.0,
        &[0.05, 0.10, 0.20],
    )?;
    println!(
        "Near-wall reverse flow: OpenFOAM separation x={:?}, reattachment x={:?}; Rust separation x={:?}, reattachment x={:?}",
        report.reference.separation_x,
        report.reference.reattachment_x,
        report.computed.separation_x,
        report.computed.reattachment_x
    );
    let mut csv = String::from("requested_x,sampled_x,y,openfoam_Ux,rust_Ux,abs_error\n");
    for profile in &report.computed.profiles {
        println!(
            "Profile x={:.4} (sampled {:.4}): rel_l2={:.4e}, max_abs={:.4e}",
            profile.requested_x, profile.sampled_x, profile.relative_l2, profile.max_abs_error
        );
        for i in 0..profile.y.len() {
            csv.push_str(&format!(
                "{:.9},{:.9},{:.9},{:.12e},{:.12e},{:.12e}\n",
                profile.requested_x,
                profile.sampled_x,
                profile.y[i],
                profile.reference_ux[i],
                profile.computed_ux[i],
                (profile.reference_ux[i] - profile.computed_ux[i]).abs()
            ));
        }
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, csv)?;
    println!("Profile CSV written to: {}", output.display());
    Ok(ExitCode::SUCCESS)
}

fn cmd_generate_reference(
    case: &str,
    corpus_dir: &Path,
    ref_dir: &Path,
    force: bool,
    openfoam_env: Option<&Path>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let case_src = corpus_dir.join(case);
    if !case_src.exists() {
        eprintln!("ERROR: Corpus case not found: {}", case_src.display());
        return Ok(ExitCode::from(2));
    }

    let case_ref = ref_dir.join(case);
    let existing_prov = case_ref.join("provenance.toml");
    if existing_prov.exists() && !force {
        eprintln!(
            "ERROR: Reference already exists at {} (provenance found)\n\
             Use --force to overwrite (this is dangerous and should only be done\n\
             when you have verified the new reference is correct).",
            case_ref.display()
        );
        return Ok(ExitCode::from(2));
    }

    // Check OpenFOAM availability
    let env_script = openfoam_env
        .or_else(|| Some(Path::new("/mnt/secondary/openfoam/bashof.sh")))
        .ok_or("OpenFOAM environment script not found. Use --openfoam-env to specify.")?;

    if !env_script.exists() {
        eprintln!(
            "ERROR: OpenFOAM environment script not found: {}",
            env_script.display()
        );
        eprintln!("  Cannot generate reference without OpenFOAM installed.");
        return Ok(ExitCode::from(2));
    }

    println!("Generating reference for case '{}'...", case);
    println!("  Source:  {}", case_src.display());
    println!("  Target:  {}", case_ref.display());
    println!("  Env:     {}", env_script.display());

    // Create a temporary work directory
    let work_dir = std::env::temp_dir().join(format!("foamcore_ref_{}", case));
    if work_dir.exists() {
        std::fs::remove_dir_all(&work_dir)?;
    }
    std::fs::create_dir_all(&work_dir)?;

    // Copy case files
    copy_dir_recursive(&case_src, &work_dir)?;

    // Rename U.orig -> U (OpenFOAM convention)
    let u_orig = work_dir.join("0").join("U.orig");
    let u_file = work_dir.join("0").join("U");
    if u_orig.exists() {
        std::fs::rename(&u_orig, &u_file)?;
    }

    // Generate mesh if blockMeshDict exists and no polyMesh yet
    let block_mesh = work_dir.join("system").join("blockMeshDict");
    let poly_mesh = work_dir.join("constant").join("polyMesh");
    if block_mesh.exists() && !poly_mesh.exists() {
        println!("  Generating mesh with blockMesh...");
        let run_cmd = format!(
            "source {} && cd {} && blockMesh 2>&1",
            env_script.display(),
            work_dir.display()
        );
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&run_cmd)
            .output()?;
        if !output.status.success() {
            eprintln!(
                "ERROR: blockMesh failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::fs::remove_dir_all(&work_dir)?;
            return Ok(ExitCode::from(2));
        }
    } else if !poly_mesh.exists() {
        eprintln!("ERROR: No mesh found (no blockMeshDict and no constant/polyMesh).");
        eprintln!("  The case needs either a blockMeshDict or a pre-generated mesh.");
        std::fs::remove_dir_all(&work_dir)?;
        return Ok(ExitCode::from(2));
    }

    // Run potentialFoam with writePhi to get the potential field
    println!("  Running potentialFoam...");
    let run_cmd = format!(
        "source {} && cd {} && potentialFoam -writePhi 2>&1",
        env_script.display(),
        work_dir.display()
    );
    let start = std::time::Instant::now();
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&run_cmd)
        .output()?;
    let elapsed = start.elapsed().as_secs_f64();

    if !output.status.success() {
        eprintln!(
            "ERROR: potentialFoam failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::remove_dir_all(&work_dir)?;
        return Ok(ExitCode::from(2));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("  OpenFOAM output (last 5 lines):");
    for line in stdout.lines().rev().take(5) {
        println!("    {line}");
    }

    // Collect results into reference directory
    std::fs::create_dir_all(&case_ref)?;

    // Copy U, phi, Phi from the latest time directory
    let time_dir = find_latest_time_dir(&work_dir);
    if let Some(td) = &time_dir {
        for field_name in &["U", "phi", "Phi"] {
            let src = td.join(field_name);
            if src.exists() {
                std::fs::copy(&src, case_ref.join(field_name))?;
                println!("  Captured: {}", field_name);
            }
        }
    } else {
        eprintln!("WARNING: Could not find output time directory.");
    }

    // Record provenance
    let input_checksum = directory_checksum(&case_src);
    let mesh_checksum = if work_dir.join("constant").join("polyMesh").exists() {
        directory_checksum(&work_dir.join("constant").join("polyMesh"))
    } else {
        None
    };

    // Try to get OpenFOAM git commit from known source locations
    let candidate_sources = [
        PathBuf::from("/mnt/secondary/openfoam/OpenFOAM-14"),
        dirs_home_openfoam(),
    ];
    let of_source = candidate_sources.iter().find(|p| p.join(".git").exists());
    let (commit, dirty) = if let Some(src) = of_source {
        let commit_out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(src)
            .output();
        let dirty_out = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(src)
            .output();
        let commit = commit_out
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let dirty = dirty_out
            .ok()
            .filter(|o| o.status.success())
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        (commit, dirty)
    } else {
        (String::new(), false)
    };

    let prov = Provenance {
        format_version: 1,
        solver: "potentialFoam".into(),
        distribution: "OpenFOAM Foundation".into(),
        version: "14".into(),
        commit,
        dirty_tree: dirty,
        precision: "float64".into(),
        platform: "linux64GccDPInt32Opt".into(),
        compiler: Some("GCC (DPInt32Opt)".into()),
        case_name: case.to_string(),
        input_checksum,
        mesh_checksum,
        generated_at: foamcore_validation::provenance::chrono_now_public(),
        execution_time_s: Some(elapsed),
    };
    prov.write(&case_ref.join("provenance.toml"))?;
    println!("  Provenance recorded.");

    // Cleanup work directory
    std::fs::remove_dir_all(&work_dir)?;

    println!("\nReference generation complete for '{}'.", case);
    Ok(ExitCode::SUCCESS)
}

fn cmd_check_provenance(
    ref_dir: &Path,
    case: &str,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let prov_path = ref_dir.join(case).join("provenance.toml");
    if !prov_path.exists() {
        eprintln!("ERROR: No provenance found at {}", prov_path.display());
        return Ok(ExitCode::from(1));
    }

    let prov = Provenance::read(&prov_path)?;
    println!("Provenance for '{}':", case);
    println!(
        "  Solver:       {} v{} ({})",
        prov.solver, prov.version, prov.distribution
    );
    println!(
        "  Commit:       {}",
        if prov.commit.is_empty() {
            "unknown"
        } else {
            &prov.commit
        }
    );
    println!("  Dirty tree:   {}", prov.dirty_tree);
    println!("  Precision:    {}", prov.precision);
    println!("  Platform:     {}", prov.platform);
    println!(
        "  Input hash:   {}",
        prov.input_checksum.as_deref().unwrap_or("not recorded")
    );
    println!(
        "  Mesh hash:    {}",
        prov.mesh_checksum.as_deref().unwrap_or("not recorded")
    );
    println!("  Generated:    {}", prov.generated_at);
    if let Some(t) = prov.execution_time_s {
        println!("  Run time:     {:.2}s", t);
    }

    // Verify the recorded input against the case source used in this repo.
    if let Some(hash) = &prov.input_checksum {
        let source_case = [
            PathBuf::from("corpus").join(case),
            PathBuf::from("icoFoamRust/cases").join(case),
        ]
        .into_iter()
        .find(|path| path.exists());
        if let Some(source_case) = source_case {
            let current_hash = directory_checksum(&source_case).unwrap_or_default();
            if current_hash != *hash {
                eprintln!("\nWARNING: Input case has been modified since reference was generated!");
                eprintln!("  Source:   {}", source_case.display());
                eprintln!("  Recorded: {hash}");
                eprintln!("  Current:  {current_hash}");
                return Ok(ExitCode::from(1));
            } else {
                println!(
                    "\n  Input checksum verified OK against {}.",
                    source_case.display()
                );
            }
        } else {
            println!("\n  Input checksum not verified: no source case directory found.");
        }
    }

    if let Some(hash) = &prov.mesh_checksum {
        let mesh_source = [
            PathBuf::from("corpus").join(case),
            PathBuf::from("icoFoamRust/cases").join(case),
        ]
        .into_iter()
        .map(|path| path.join("constant").join("polyMesh"))
        .find(|path| path.exists());
        if let Some(mesh_source) = mesh_source {
            let current_hash = directory_checksum(&mesh_source).unwrap_or_default();
            if current_hash != *hash {
                eprintln!("\nWARNING: Mesh has been modified since reference was generated!");
                eprintln!("  Source:   {}", mesh_source.display());
                eprintln!("  Recorded: {hash}");
                eprintln!("  Current:  {current_hash}");
                return Ok(ExitCode::from(1));
            } else {
                println!(
                    "  Mesh checksum verified OK against {}.",
                    mesh_source.display()
                );
            }
        } else {
            println!("  Mesh checksum not verified: no source mesh directory found.");
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Find ~/github/OpenFOAM-14 as a fallback source location.
fn dirs_home_openfoam() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join("github/OpenFOAM-14")
}

/// Find the output time directory. Steady-state solvers write to "0/",
/// transient solvers write to numbered directories.
fn find_latest_time_dir(work_dir: &Path) -> Option<PathBuf> {
    let mut max_t: f64 = -1.0;
    let mut best: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(work_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(t) = name.parse::<f64>() {
                if t > max_t {
                    max_t = t;
                    best = Some(entry.path());
                }
            }
        }
    }
    // If no positive time dir found, check "0" (steady-state output)
    if best.is_none() {
        let zero_dir = work_dir.join("0");
        if zero_dir.exists() && zero_dir.is_dir() {
            return Some(zero_dir);
        }
    }
    best
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
