# OpenFlowSim

OpenFlowSim is a work in progress to implement OpenFOAM 14's `potentialFoam`
solver in Rust and validate its results against OpenFOAM and analytical cases.
The repository currently contains the OpenFOAM solver reference, case inputs,
one generated reference dataset, and a Rust validation tool. **The Rust solver
has not been implemented yet.**

## Project status

### Done

- Added the OpenFOAM 14 `potentialFoam` source and supporting headers under
  [`potentialFoam/`](potentialFoam/).
- Added three case setups: `simpleChannel`, `cylinder`, and `pitzDaily` under
  [`corpus/`](corpus/).
- Generated OpenFOAM reference fields (`U`, `phi`, and `Phi`) and provenance
  metadata for `simpleChannel` under [`reference/simpleChannel/`](reference/simpleChannel/).
- Built the Rust `foamcore-validation` crate. It parses OpenFOAM scalar and
  vector internal fields, compares fields with tolerances, checks reference
  provenance, and writes JSON and Markdown reports.
- Added a uniform-channel analytical check for `U` and `Phi`.
- Verified the harness with 33 passing Rust tests. The analytical check passes
  against the stored OpenFOAM simple-channel result.

### In progress / not done

- There is no Rust solver executable or implementation of the mesh, finite
  volume operations, boundary conditions, linear system solve, or flux
  reconstruction.
- Only `simpleChannel` currently has generated OpenFOAM reference fields.
- `cylinder` and `pitzDaily` are case inputs, but they do not yet have usable
  solver results and reference data in this repository.
- `compare-all` writes separate per-case reports. It does not yet produce one
  combined report.

## Repository layout

| Path | Purpose |
| --- | --- |
| `potentialFoam/` | OpenFOAM 14 C++ solver source used as the implementation reference |
| `corpus/` | OpenFOAM case inputs and metadata |
| `reference/` | Stored OpenFOAM output fields and provenance |
| `results/` | Location for future Rust solver output (ignored by Git) |
| `validation/` | Rust validation crate, tolerances, tests, and documentation |

## Build and validate

Check that the Rust toolchain, Python, uv, and the project `.venv` are ready:

```sh
./checkreqs.sh
```

When requirements are missing, the script prints setup commands for Ubuntu.

From the repository root, run the validation tests:

```sh
cargo test
```

Compare Rust output in `results/<case>/` with stored OpenFOAM fields:

```sh
cargo run --bin foamcore-validate -- compare simpleChannel
cargo run --bin foamcore-validate -- compare-all
```

Run the analytical uniform-channel check:

```sh
cargo run --bin foamcore-validate -- analytical simpleChannel
```

Reports are written under `validation/reports/`. The comparison command expects
solver results to be placed in `results/<case>/`; without them, the case is
reported as an error. See [`validation/README.md`](validation/README.md) for
details on the harness and its current limitations.

After generating validation JSON reports, create a browsable HTML summary with:

```sh
./genreport.sh
```

The page is written to `validation/reports/report.html` by default. An alternate
output path can be passed as the first argument. The HTML is self-contained and
can be opened in a browser or printed to PDF.

To regenerate the simple-channel OpenFOAM reference, install/source an
OpenFOAM 14 environment and run:

```sh
cargo run --bin foamcore-validate -- generate-reference simpleChannel
```

Reference generation protects existing data. Use the command's `--force`
option only when intentionally replacing a reference.

## Testing and validation roadmap

Treat verification and validation as related but distinct tasks. **Verification**
asks whether the code solves the intended discrete equations correctly.
**Validation** asks whether those equations and modeling choices describe
physical behavior, using suitable experimental or benchmark data. OpenFOAM is a
useful implementation reference, but agreement with it alone does not establish
physical correctness.

Build confidence in stages:

1. Test numerical building blocks against known answers with tight tolerances.
2. For `potentialFoam`, use small deterministic cases and analytical results
   where available. Compare fields with stated tolerances, and check flux or
   mass conservation and solver convergence.
3. For steady laminar cases, compare field norms alongside quantities such as
   velocity profiles and pressure drop. Add mesh refinement checks to see
   whether results converge as the mesh is refined.
4. For unsteady flows, compare early-time fields where useful, then compare
   time histories, shedding frequencies, forces, and conservation measures.
5. For chaotic or turbulent flows, do not require long-time cell-by-cell
   agreement. Compare statistical quantities, such as mean fields and energy
   spectra, over documented sampling windows and uncertainty ranges.

Use visualizations to spot artifacts and implausible flow patterns. They are a
diagnostic aid alongside numerical checks, not a substitute for them. Start
with the existing `simpleChannel` analytical check and OpenFOAM reference;
add benchmark cases and references as each new validation level is implemented.

## Solver behavior to port

The main OpenFOAM flow in `potentialFoam/potentialFoam.C` is:

1. Read `U`, initialize it to zero, and form the face flux `phi`.
2. Infer pressure and velocity-potential boundary conditions from the input
   fields.
3. Apply rotating-frame and boundary-flux adjustments.
4. Solve the `Phi` Laplace equation, correcting `phi` with the equation flux.
5. Reconstruct `U` from the corrected flux and correct its boundary values.
6. Optionally calculate and write pressure; write the resulting fields.

The Rust implementation should be developed against this flow and validated
incrementally with the stored reference and analytical checks. The simple
channel provides the first end-to-end case; additional references are needed
before the other corpus cases can serve as regression tests.
