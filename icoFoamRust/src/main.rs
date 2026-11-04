use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "foamcore-icofoam", about = "Rust icoFoam implementation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Load and display case setup from an OpenFOAM case directory.
    Inspect {
        /// Case directory containing system/controlDict, fvSchemes, fvSolution.
        case: PathBuf,
    },
    /// Run the icoFoam solver (not yet implemented).
    Run {
        /// Case directory to solve.
        case: PathBuf,
    },
}

fn main() -> ExitCode {
    match cli_main(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cli_main(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Inspect { case } => {
            let setup = foamcore_icofoam::CaseSetup::read(&case)?;
            print_setup(&setup);
        }
        Command::Run { case } => {
            let _setup = foamcore_icofoam::CaseSetup::read(&case)?;
            println!("Solver not yet implemented. Case loaded successfully from {}", case.display());
        }
    }
    Ok(())
}

fn print_setup(setup: &foamcore_icofoam::CaseSetup) {
    let c = &setup.control;
    println!("=== controlDict ===");
    println!("  startTime:     {}", c.start_time);
    println!("  endTime:       {}", c.end_time);
    println!("  deltaT:        {}", c.delta_t);
    println!("  numSteps:      {}", c.num_steps());
    println!(
        "  writeControl:  {:?}",
        c.write_control
    );
    println!("  writeInterval: {}", c.write_interval);
    println!("  writeFormat:   {}", c.write_format);

    let s = &setup.schemes;
    println!("\n=== fvSchemes ===");
    println!("  ddt:           {}", s.ddt_default);
    println!("  grad (default):{}", s.grad_default.join(" "));
    for (expr, scheme) in &s.div_schemes {
        println!("  {}:     {}", expr, scheme.join(" "));
    }
    println!(
        "  laplacian:     {}",
        s.laplacian_default.join(" ")
    );
    println!("  interpolation: {}", s.interpolation_default);

    let f = &setup.solution;
    println!("\n=== fvSolution ===");
    for solver in &f.solvers {
        println!(
            "  {}: {} tol={:.1e} relTol={}",
            solver.field, solver.solver, solver.tolerance, solver.rel_tol
        );
    }
    println!(
        "\n  PISO: nCorrectors={}, nNonOrthogonalCorrectors={}, pRefCell={}, pRefValue={}",
        f.piso.n_correctors,
        f.piso.n_non_orthogonal_correctors,
        f.piso.p_ref_cell,
        f.piso.p_ref_value
    );
}
