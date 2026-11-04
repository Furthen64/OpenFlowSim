# Task 05: PISO Algorithm & Time Loop

## Goal
Implement the main icoFoam time-stepping loop with the PISO pressure-velocity coupling.

## Algorithm (per time step)
```
1. READ U, p from previous time step

2. MOMENTUM PREDICTOR:
   Assemble: A_U * U = -div(phi,U) + laplacian(nu,U) - grad(p)
   Solve:    U_new = solve(A_U, rhs)
   (or skip if momentumPredictor = false in pisoControl)

3. PISO CORRECTOR LOOP (nCorrectors times, default 2):
   a. rAU  = 1 / diagonal(A_U)
   b. HbyA = rAU * (rhs - offDiag * U)     // "U without pressure"
   c. phiHbyA = flux(HbyA) + interpolate(rAU)*ddtCorr(U, phi)
   d. adjustPhi(phiHbyA)                    // enforce BCs on flux
   e. constrainPressure(p, U, phiHbyA, rAU) // update p BCs for consistency

   f. NON-ORTHOGONAL CORRECTOR LOOP (nNonOrthogonalCorrectors, can be 0):
      Assemble: A_p * p = div(phiHbyA)     // pressure Poisson
      Set reference cell/value
      Solve for p
      If final iteration: phi = phiHbyA - flux(p)

   g. U = HbyA - rAU * grad(p)
      U.correctBoundaryConditions()

4. WRITE fields to <time>/ directory
5. ADVANCE time
```

## Deliverables
- Time loop with proper deltaT handling (fixed to start)
- Momentum equation assembly + solve (reuses Task 03 operators)
- PISO corrector loop with proper phi management
- Pressure equation assembly + solve (scalar Poisson, reuses CG/GMRES)
- Pressure reference cell fixation (gauge setting)
- Continuity error reporting (sum of div(phi) over domain)
- Courant number calculation + reporting

## Notes
- Start with nCorrectors=2, nNonOrthogonalCorrectors=0 (orthogonal mesh assumption)
- The ddtCorr term is zero on the first corrector; only matters for subsequent ones
- For the very first time step, ddt(U) = (U - U0)/dt where U0 is from `0/` directory
- Pressure reference: findRefCell picks a cell, sets p[cell] = pRefValue (usually 0)
- This is where most bugs will live — validate each sub-step against OpenFOAM output
