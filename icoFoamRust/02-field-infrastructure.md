# Task 02: Field Infrastructure Extensions

## Goal
Extend the existing mesh/field code to support everything icoFoam needs beyond potentialFoam.

## Deliverables
- Scalar volume field reader/writer (`volScalarField`) — pressure `p`, `rAU`
- Vector volume field reader already exists; ensure it handles time directories (`<time>/U`)
- Surface scalar field reader/writer (`surfaceScalarField`) — the flux `phi`
- Time directory management: read initial fields from `0/`, write results to `<t>/`
- Support for reading `constant/transportProperties` (nu value)

## Notes
- The existing `solver/src/field.rs` handles vector fields; extend or parallel with scalar
- phi lives on faces, not cells — different storage layout (nFaces vs nCells)
- Boundary patches for scalars differ from vectors (e.g., `fixedFluxPressure` for p at outlet)
- Consider whether to share code with `solver/` crate via a common lib, or duplicate initially
