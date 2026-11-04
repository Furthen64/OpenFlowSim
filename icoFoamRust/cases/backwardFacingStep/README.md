# Backward-facing step

This is a structured 900-cell backward-facing step mesh with orthogonal cells and a 2D empty front/back span. The inlet is 10 m/s, all walls including the step are no-slip, the inlet pressure is zero-gradient, the outlet pressure is fixed at zero, and `nu = 0.001 m^2/s`. The case advances to `t = 0.01` with `deltaT = 0.0001`.

Generate and run the OpenFOAM 14 reference with `blockMesh && icoFoam`. The Rust comparison passes the `1e-5` relative L2 target for all three fields: `U = 2.708e-11`, gauge-aligned `p = 9.812e-9`, and `phi = 1.607e-10`.

The near-wall line detects separation at the step and reattachment at `x = 0.047125 m` in both solvers at `t = 0.01`. Streamwise velocity profiles at three downstream stations are exported to `icoFoamRust/validation/backwardFacingStep_profiles.csv`.
