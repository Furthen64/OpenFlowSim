# Plane channel / Poiseuille flow

This 40 × 10 × 1 channel has a uniform fixed-velocity inlet, zero-pressure outlet, no-slip walls, and `nu = 1 m^2/s` (Re = 1 on the unit channel height). The run advances to `t = 5` with `deltaT = 0.01`; this gives a low-Reynolds-number developing channel profile suitable for comparison with the parallel-plate Poiseuille solution.

Generate or run the OpenFOAM reference with `blockMesh && icoFoam`. The mesh is also checked in because `foamcore-icofoam` currently reads `constant/polyMesh` directly.
