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
