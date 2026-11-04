# Task 01: Project Scaffolding & Case Reading

## Goal
Set up the `icoFoamRust` crate structure and implement reading of OpenFOAM case setup files.

## Deliverables
- New cargo binary/library crate (or module within existing workspace)
- Parser for `system/controlDict` (endTime, deltaT, writeControl, etc.)
- Parser for `system/fvSchemes` (ddtScheme, divSchemes, gradSchemes, laplacianSchemes)
- Parser for `system/fvSolution` (solver choices, tolerances, nCorrectors, nNonOrthogonalCorrectors)
- CLI entry point: `run <case_dir>` that loads all three and prints parsed config

## Notes
- Reuse patterns from `solver/` crate where possible (mesh.rs, field.rs)
- fvSchemes/fvSolution can start with hardcoded defaults; full parsing is a stretch goal
- controlDict fields we need: startTime, endTime, deltaT, controlFormat, writeInterval
