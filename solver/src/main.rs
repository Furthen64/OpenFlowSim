use clap::{Parser, Subcommand};
use foamcore_solver::field::VolumeVectorField;
use foamcore_solver::flow;
use foamcore_solver::mesh::PolyMesh;
use foamcore_solver::output;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "foamcore-solver", about = "Rust potentialFoam implementation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the orthogonal-mesh potential solver for an OpenFOAM case.
    Run {
        /// Case directory containing constant/polyMesh and 0/U (or 0/U.orig).
        case: PathBuf,
        /// Output case directory (defaults to results/<case-name>).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Output time directory name.
        #[arg(long, default_value = "1")]
        time: String,
    },
    /// Read and validate an ASCII OpenFOAM polyMesh and report its geometry.
    InspectMesh {
        /// OpenFOAM case directory containing constant/polyMesh.
        case: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ERROR: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Run {
            case,
            output: output_dir,
            time,
        } => {
            let mesh_path = case.join("constant/polyMesh");
            let mesh = if mesh_path.is_dir() {
                PolyMesh::read_case(&case)?
            } else {
                foamcore_solver::structured::from_case(&case)?
            };
            let geometry = mesh.geometry()?;
            let input_path = [case.join("0/U"), case.join("0/U.orig")]
                .into_iter()
                .find(|path| path.is_file())
                .ok_or_else(|| {
                    format!(
                        "no U field found under {}/0 (expected U or U.orig)",
                        case.display()
                    )
                })?;
            let input_u = VolumeVectorField::read(&input_path)?;
            let solution = flow::solve(&mesh, &geometry, &input_u)?;
            let case_name = case
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("case");
            let output_dir = output_dir.unwrap_or_else(|| Path::new("results").join(case_name));
            let time_dir = output_dir.join(&time);
            output::write_solution(&mesh, &input_u, &solution, &time_dir, &time)?;
            println!(
                "Solved {} cells on {} internal faces",
                geometry.cells.len(),
                mesh.internal_face_count()
            );
            println!("Wrote U, phi, and Phi to {}", time_dir.display());
        }
        Command::InspectMesh { case } => {
            let mesh_path = case.join("constant/polyMesh");
            let mesh = if mesh_path.is_dir() {
                PolyMesh::read_case(&case)?
            } else {
                foamcore_solver::structured::from_case(&case)?
            };
            let geometry = mesh.geometry()?;
            println!("Case: {}", case.display());
            println!("Cells: {}", geometry.cells.len());
            println!(
                "Faces: {} ({} internal)",
                geometry.faces.len(),
                mesh.internal_face_count()
            );
            println!("Points: {}", mesh.points.len());
            println!("Boundary patches:");
            for patch in &mesh.patches {
                println!(
                    "  {}: {} faces ({})",
                    patch.name, patch.face_count, patch.patch_type
                );
            }
            let min_volume = geometry
                .cells
                .iter()
                .map(|cell| cell.volume)
                .fold(f64::INFINITY, f64::min);
            let max_volume = geometry
                .cells
                .iter()
                .map(|cell| cell.volume)
                .fold(0.0, f64::max);
            println!("Cell volume range: {min_volume:.8e} .. {max_volume:.8e}");
        }
    }
    Ok(())
}
