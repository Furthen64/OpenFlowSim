# Lid-driven cavity

OpenFOAM 14 input and generated 40 × 40 × 1 orthogonal mesh for the task 06 cavity case.
The run stops at `t = 1.0` with `deltaT = 0.005`. This resolves the primary circulation and four local corner extrema while remaining under the task's 10k-cell limit. Solver tolerances are tightened to `1e-12` so OpenFOAM's iterative solution matches the Rust solver's converged result over the full transient.

The reference was generated from this input with OpenFOAM Foundation 14 `icoFoam`:

```sh
blockMesh
icoFoam
```

The `div(phi,U)` scheme is `Gauss linear` to match the current Rust discretization.
