# Task 03: Finite Volume Operators

## Goal
Implement the discrete FV operators that appear in the icoFoam equations.

## Equations to support
```
fvm::ddt(U)            // (U - U0) / dt          [vector]
fvm::div(phi, U)       // convective flux         [vector, given scalar phi]
fvm::laplacian(nu, U)  // viscous diffusion       [scalar nu, vector U]
fvm::laplacian(rAU, p) // pressure Poisson        [scalar rAU, scalar p]
fvc::grad(p)           // pressure gradient       [scalar -> vector]
fvc::flux(HbyA)        // interpolate + face flux [vector -> surface]
fvc::interpolate(rAU)  // cell -> face interpolation
fvc::div(phiHbyA)      // face divergence         [surface -> volume scalar]
```

## Deliverables
- `ddt` operator (explicit Euler to start): returns source term + modifies matrix diagonal
- `div(phi, U)` with upwind or linear scheme: assembles matrix contribution + handles BCs
- `laplacian(D, phi)` for scalar D and scalar/vector phi: assembles sparse matrix (off-diagonals from face interpolation)
- `fvc::grad`, `fvc::div`, `fvc::flux`, `fvc::interpolate` as explicit (non-matrix) operations
- All operators must work on the existing orthogonal mesh structure

## Notes
- Start with linear (central-difference) schemes; upwind is a refinement
- The laplacian assembly is the core: for each internal face, off-diagonal = -D * area / distance
- Boundary faces contribute to the source vector, not off-diagonals
- Reuse the sparse matrix + CG/GMRES solver from `solver/src/flow.rs`
- Vector laplacian = 3 independent scalar laplacians (one per component) in FV
