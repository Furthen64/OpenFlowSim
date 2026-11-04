# icoFoamRust

This crate contains the orthogonal-mesh icoFoam implementation. Run a case with:

```sh
cargo run -p foamcore-icofoam -- run icoFoamRust/cases/cavity
```

The checked cavity case uses a 40 × 40 × 1 mesh, Euler time stepping, linear convection, and a fixed time step of 0.005 through `t = 1.0`. Its OpenFOAM Foundation 14 reference fields are in `reference/cavity/1/`; provenance is recorded alongside them. The streamfunction check confirms matching primary-vortex centers and four resolved corner extrema.

The `pipeFlow` case is a 40 × 10 × 1 low-Reynolds-number plane channel advanced to `t = 5`. Its OpenFOAM 14 `icoFoam` reference is in `reference/pipeFlow/5/`.

The `backwardFacingStep` case uses a 900-cell structured orthogonal step mesh and an OpenFOAM 14 `icoFoam` reference in `reference/backwardFacingStep/0.01/`.

To validate a run with the existing harness, copy the output time directory under a results case directory and run:

```sh
cargo run -p foamcore-validation -- compare cavity \
  --results-dir /path/to/results \
  --ref-dir icoFoamRust/reference
```

The validation harness checks relative L2 error for `U`, gauge-aligned `p`, and `phi` against the `1e-5` target. All three cases pass. Pipe flow also has an analytical outlet-profile check.

The cavity streamfunction diagnostic reports relative-L2 `7.898e-7`, matching primary-vortex centers at `(0.0788, 0.0387) m`, and four matching corner extrema. Across 200 Rust time steps, the maximum volume-integrated local continuity error is `5.021e-14` (final `2.725e-14`); maximum Courant is `0.9205`.

To write SVG comparisons of reference and Rust cell-centred velocity magnitude and pressure, add `--plots-dir` and identify the case mesh:

```sh
cargo run -p foamcore-validation -- compare backwardFacingStep \
  --results-dir icoFoamRust/cases \
  --ref-dir icoFoamRust/reference \
  --mesh-case icoFoamRust/cases/backwardFacingStep \
  --plots-dir validation/reports/plots
```

The SVGs show reference, Rust result, and absolute error panels on the mesh cell centres.

Export any OpenFOAM case boundary mesh to Wavefront OBJ with `export-obj`. The
case must contain an ASCII `constant/polyMesh`. By default, all boundary patches
are included as OBJ groups; pass comma-separated patch names to select only some
of them:

```sh
cargo run -p foamcore-validation -- export-obj \
  --case icoFoamRust/cases/backwardFacingStep \
  --output icoFoamRust/geometry/backwardFacingStep.obj

cargo run -p foamcore-validation -- export-obj \
  --case /path/to/openfoam/example \
  --output /path/to/example.obj \
  --patches inlet,outlet
```

The OBJ contains the selected boundary polygons and their vertices, with patch
names preserved as groups. Internal faces are not exported.

The backward-facing-step near-wall separation/reattachment and sampled `Ux` profiles can be checked and exported with:

```sh
cargo run -p foamcore-validation -- step-diagnostics \
  --results-dir /path/to/results \
  --ref-dir icoFoamRust/reference \
  --mesh-case icoFoamRust/cases/backwardFacingStep \
  --output validation/reports/backwardFacingStep_profiles.csv
```

For the checked `t = 0.01` run, both solvers detect separation at the step (`x = 0`) and reattachment at `x = 0.047125 m`. The three profile relative-L2 errors at `x ≈ 0.0471`, `0.0979`, and `0.1994 m` are `4.584e-12`, `5.079e-12`, and `2.536e-11`; sampled values are stored in `validation/backwardFacingStep_profiles.csv`.

Check the pipe's outlet profile against the parallel-plate Poiseuille solution with:

```sh
cargo run -p foamcore-validation -- analytical pipeFlow \
  --results-dir /path/to/results
```
