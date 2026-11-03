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

- The first runnable Rust `potentialFoam` path is implemented for orthogonal
  meshes and the `simpleChannel` case. It solves for `Phi`, reconstructs `U`,
  and writes `U`, `phi`, and `Phi`. General non-orthogonal correction, broader
  boundary conditions, and pressure output remain future work.
- Only `simpleChannel` currently has generated OpenFOAM reference fields.
- `cylinder` and `pitzDaily` are case inputs, but they do not yet have usable
  solver results and reference data in this repository.
- `compare-all` writes separate per-case reports. It does not yet produce one
  combined report.

### Corpus validation pipeline

`validate_corpus.sh` runs supported cases end to end: it runs the Rust solver,
compares outputs with matching references, performs applicable analytical
checks, and writes aggregate JSON and Markdown reports under a unique
`validation/runs/<run-id>/` directory. Unsupported cases and cases without
matching references are recorded as skipped with a reason. The command returns
failure if any executed case fails; `genreport.sh` can render a run's reports
as HTML.

### Planned: Rust `potentialFoam` solver

Use [`potentialFoam/potentialFoam.C`](potentialFoam/potentialFoam.C) and its
included field/control files as the behavioral reference. Build the solver in
small stages, beginning with the existing `simpleChannel` case and expanding
to general OpenFOAM meshes and boundary conditions after the basic numerical
path is validated.

- [x] **Define the first executable contract.** Add a Rust solver binary with
  `run <case>` that reads `U`, writes OpenFOAM fields in a results time
  directory, and returns a failing exit code on invalid input or a failed
  solve. Optional pressure output and broader OpenFOAM command-line
  compatibility remain deferred.
- [ ] **Represent and load meshes.** Define mesh cells, faces, owner/neighbour
  addressing, face area vectors, cell centres and volumes, and boundary
  patches. Read OpenFOAM `polyMesh` files and validate their addressing and
  geometry. First exercise this on a generated `simpleChannel` mesh; then add
  non-orthogonal mesh support needed by the other cases.
- [ ] **Read and write fields.** Parse the required scalar and vector volume
  fields, dimensions, internal values, patch types, and patch values. Preserve
  or correctly emit OpenFOAM field syntax so solver output can be read by
  OpenFOAM and the existing Rust validation crate. Add focused round-trip and
  malformed-input tests.
- [ ] **Implement boundary and flux setup.** Support the initial velocity
  boundary conditions used by `simpleChannel`, infer potential boundary
  conditions from pressure boundary conditions as in `createFields.H`, form
  face fluxes from velocity, and enforce net boundary flux consistency.
- [ ] **Assemble and solve the potential equation.** Implement finite-volume
  divergence and Laplacian operators, reference-cell handling for the
  arbitrary potential gauge, linear-system assembly/solve, and
  non-orthogonal correction loops. Check conservation, residuals, and
  flux-correction sign conventions against the bundled OpenFOAM source.
- [ ] **Correct flux and reconstruct velocity.** Apply the solved potential's
  face-flux correction, reconstruct cell velocity from face fluxes, enforce
  velocity boundary conditions, and report continuity and interpolation
  errors.
- [ ] **Validate the MVP end to end.** Run `simpleChannel`; check its uniform
  analytical velocity and potential (up to gauge), compare `U`, `phi`, and
  `Phi` with the stored OpenFOAM reference, and run the planned corpus pipeline.
  Keep each numerical operator covered by small deterministic unit tests.
- [ ] **Expand case and feature coverage.** Add robust non-orthogonal and
  boundary-condition support, then validate `cylinder`. Make the missing mesh
  and reference requirements for `pitzDaily` explicit before claiming it as
  covered. Consider optional approximate pressure (`-writep`), rotating-frame
  zones, and broader OpenFOAM compatibility only after the core solver passes
  its regression cases.

## Repository layout

| Path | Purpose |
| --- | --- |
| `potentialFoam/` | OpenFOAM 14 C++ solver source used as the implementation reference |
| `corpus/` | OpenFOAM case inputs and metadata |
| `reference/` | Stored OpenFOAM output fields and provenance |
| `results/` | Location for future Rust solver output (ignored by Git) |
| `solver/` | Rust `potentialFoam` implementation (in progress) |
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

Run the Rust potential solver for the supported channel case:

```sh
cargo run -p foamcore-solver -- run corpus/simpleChannel
```

The command writes fields to `results/simpleChannel/1/`. It reads an existing
ASCII `constant/polyMesh` when present; otherwise, it builds the current
structured channel mesh from `case.toml`. The solver currently supports
orthogonal meshes and the case's fixed-velocity, zero-gradient, symmetry, and
empty boundary conditions.

Compare Rust output in `results/<case>/` with stored OpenFOAM fields:

```sh
cargo run --bin foamcore-validate -- compare simpleChannel
cargo run --bin foamcore-validate -- compare-all
```

Run the analytical uniform-channel check:

```sh
cargo run --bin foamcore-validate -- analytical simpleChannel
```

Run the full supported corpus pipeline:

```sh
./validate_corpus.sh
```

Inspect corpus metadata and data availability without running anything:

```sh
./corpus_overview.sh
```

The table shows whether a generated `polyMesh` exists, which mesh-generation
inputs are present, available initial/reference fields, and missing inputs.

Use `--run-dir DIR` to select a new output directory or `--no-html` to skip
the optional HTML rendering. Each run includes solver/comparison logs,
per-case JSON and Markdown, and aggregate `reports/corpus-validation.json` and
`reports/corpus-validation.md` files.

Render an existing run again with:

```sh
./genreport.sh --reports-dir validation/runs/<run-id>/reports \
  --output validation/runs/<run-id>/report.html
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
