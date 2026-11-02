# OpenFlowSim validation harness

This crate validates OpenFOAM field files and will be used to check a Rust
implementation of `potentialFoam`. It does not contain the Rust flow solver.

## Commands

Run the synthetic parser and comparison tests from the workspace root:

```sh
cargo test
```

Compare solver output under `results/<case>/` with the checked reference under
`reference/<case>/`:

```sh
cargo run --bin foamcore-validate -- compare simpleChannel
cargo run --bin foamcore-validate -- compare-all
```

Run the independent uniform-channel analytical check after placing `U` (and,
optionally, `Phi`) in `results/simpleChannel/`:

```sh
cargo run --bin foamcore-validate -- analytical simpleChannel
```

Generate an OpenFOAM reference (requires OpenFOAM 14 and its environment):

```sh
cargo run --bin foamcore-validate -- generate-reference simpleChannel
```

The CLI writes JSON and Markdown reports in `validation/reports/`. Reference
data includes provenance metadata and checksums. Do not replace an existing
reference unless an intentional regeneration is desired; use the generator's
`--force` option explicitly.

## Current coverage and limitations

- OpenFOAM scalar and vector internal fields are parsed and compared.
- `Phi` comparisons account for its arbitrary additive gauge. The analytical
  channel check also sorts cell values because mesh cell ordering can differ.
- The analytical check currently uses a `1e-6` tolerance to accommodate the
  observed iterative residuals in the OpenFOAM channel result.
- `compare-all` writes a report per discovered reference case. Cases without
  corresponding solver output are reported as errors; it does not yet produce
  one combined aggregate report.
- `pitzDaily` and `cylinder` still need usable reference data and solver output
  before they can validate a Rust solver.
- No Rust mesh, finite-volume operators, linear solver, boundary-condition
  implementation, or `potentialFoam` executable is present yet.
