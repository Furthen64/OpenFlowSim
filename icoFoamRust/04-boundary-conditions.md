# Task 04: Boundary Conditions

## Goal
Implement the boundary conditions needed for standard icoFoam test cases.

## Required BCs

### Velocity U (vector)
| BC Type | Use Case | Implementation |
|---------|----------|----------------|
| `fixedValue` / `uniformFixedValue` | Inlet with specified velocity | Set face values directly |
| `noSlip` | Walls | U = 0 at wall (or U = wall motion) |
| `zeroGradient` | Outlet | Extrapolate from interior |
| `symmetryPlane` | Slip walls / periodic | Normal component = 0, tangential zeroGradient |
| `empty` | 2D edges (periodic) | No flux across, mirror values |

### Pressure p (scalar)
| BC Type | Use Case | Implementation |
|---------|----------|----------------|
| `fixedValue` | Pressure outlet (p = 0) | Set value, contributes to phi correction |
| `zeroGradient` | Inlet / walls | dp/dn = 0, affects phi via rAU * grad(p) |
| `inletOutlet` | Combined inlet/outlet | Switches based on flow direction (stretch goal) |

### Flux phi (surface scalar)
| BC Type | Use Case | Implementation |
|---------|----------|----------------|
| `zeroGradient` | Derived from U BCs | Computed, not stored independently |
| `fixedFluxPressure` | Coupled p-phi at boundaries | phi = phiHbyA - rAU * Sf . grad(p) |

## Deliverables
- BC trait/enum that can be applied during matrix assembly (modifies diagonal + source)
- BC application for explicit operations (grad, flux at boundary faces)
- `U.correctBoundaryConditions()` equivalent after each pressure update
- Wall distance / face normal handling for noSlip

## Notes
- Start with: fixedValue inlet, noSlip walls, zeroGradient outlet for U; fixedValue(0) outlet + zeroGradient elsewhere for p
- This covers: pipe flow, lid-driven cavity, backward-facing step
- The pressure BC directly affects the phi correction: phi = phiHbyA - rAU*(psi) where psi includes the BC contribution

## Research notes (verified against OpenFOAM 14 source and cavity case)

### Discretization formulas (exact OpenFOAM behaviour)

For `fvm::laplacian(D, phi)` on a boundary face (distance d, area A):
- `internalCoeffs = gamma * gradientInternalCoeffs(deltaCoeffs)`
- `boundaryCoeffs = -gamma * gradientBoundaryCoeffs(deltaCoeffs)`
- deltaCoeffs = A/d. Per BC type:
  - `zeroGradient`: gradInt=0, gradBound=0 → **no contribution at all**
  - `fixedValue(v)`: gradInt=-deltaCoeffs, gradBound=deltaCoeffs*v → diag += D*A/d, source -= D*A/d*v
  - `fixedFluxPressure(g)`: gradInt=0, gradBound=g → source -= D*g (g updated each corrector via constrainPressure)

For `fvm::div(phi, U)` on a boundary face (flux phi_f):
- `internalCoeffs = phi_f * valueInternalCoeffs(w)`
- `boundaryCoeffs = -phi_f * valueBoundaryCoeffs(w)`
- Per BC type:
  - `zeroGradient` / `noSlip` (U=0 wall): valInt=1, valBound=0 → diag += phi_f*w (w≈1 at wall)
  - `fixedValue(v)`: valInt=0, valBound=v → source -= phi_f*v

Other exact facts:
- `fvm::ddt(U)`: diag[i] = V_i/dt; source[i] = U_old[i]*V_i/dt (volume-weighted!)
- `operator==(A, field)`: source += V * field (multiplied by cell volume, not just field)
- `negSumDiag()`: for each internal face, Diag[owner] -= lower[face], Diag[neighbour] -= upper[face];
  where lower=-w*phi, upper=(1-w)*phi for div; upper=deltaCoeffs*gamma for laplacian
- `setReference(celli, value)`: source[celli] += diag[celli]*value; diag[celli] *= 2
- `rAU = 1/Ueqn.A()` where A() is the diagonal of the momentum matrix
- `HbyA = rAU * Ueqn.H()` where H() = source - offDiag*x (the "explicit" part)
- `phiHbyA = fvc::flux(HbyA) + interpolate(rAU)*ddtCorr(U, phi)` — ddtCorr is zero on first corrector
- Pressure equation: `laplacian(rAU, p) == div(phiHbyA)` — note rAU is a FIELD not constant
- `pEqn.flux()` = implicit flux from pressure solve = deltaCoeffs*(p_neigh - p_owner) per face
- Final: `phi = phiHbyA - pEqn.flux()`
- CourantNo: `0.5 * gMax(surfaceSum(mag(phi))/V) * dt`

### Cavity case specifics (/mnt/secondary/openfoam/run/cavity/)

- Patches: movingWall (U fixedValue (1,0,0)), fixedWalls (noSlip), frontAndBack (empty).
  Pressure: all zeroGradient except frontAndBack empty. Closed domain with pRefCell → adjustPhi and constrainPressure are no-ops here.
- **fvSolution uses a `PIMPLE` block, NOT `PISO`** — the parser in fv_solution.rs only looks for `"PISO"` and must accept both (values identical: nCorrectors=2, nNonOrthogonalCorrectors=0, pRefCell=0, pRefValue=0).
- Viscosity is in `constant/physicalProperties` (`nu 1e-05 [m^2/s]`), not transportProperties — physical_props.rs already checks both candidates. ✓
- Solvers: p → GAMG tol 1e-6 relTol 0.1; U → smoothSolver GaussSeidel tol 1e-5 relTol 0. Our CG solver converges tighter than these tolerances, so small differences vs OpenFOAM output are expected from its own residual tolerance.
- fvSchemes: ddt Euler; div(phi,U) `Gauss limitedLinearV 1`; laplacian `Gauss linear corrected`; interpolation linear. Mesh is orthogonal structured, so "corrected" adds nothing.

### Known bugs to fix in fv_ops.rs during this task

- `assemble_div`: boundary logic broken — uses `fv.n_internal() + fv.boundary_index_of(cell)` where `boundary_index_of` always returns 0; also treats noSlip as zeroGradient (diag += phi_f*0.5/V instead of diag += phi_f/V with w≈1).
- `fvc_grad`: ignores all BCs (assumes zeroGradient everywhere); needs fixedValue contribution at boundary faces.
- `fvc_flux_vector`: extrapolates owner value at boundaries without applying fixedValue/noSlip BCs.
- `FvMeshData::face_normal` / `owner_neighbour_to_face_index` do O(n) scans per call — precompute face index/normal arrays for performance.

### Design direction

- New `bc.rs`: enum-based BC system providing (a) per-patch coefficients for matrix assembly (internal/boundary coeffs above), (b) explicit face values for fvc operations, (c) `constrainHbyA` equivalent (set HbyA BC = U BC on non-assignable patches after each pressure update).
- Reuse existing `BoundaryPatch` type strings from scalar_field.rs parsing; map type string → BC variant.
