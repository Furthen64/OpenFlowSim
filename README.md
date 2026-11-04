# OpenFlowSim

OpenFlowSim is a Rust implementation of the potential-flow solver workflow
represented by OpenFOAM's `potentialFoam`. The project currently focuses on
one runnable solver and one supported starter case, `simpleChannel`. The
OpenFOAM source is kept as a reference; OpenFOAM itself is not required to run
the Rust solver.

## Project status

### Current scope

The project has one solver target: potential flow on finite-volume meshes,
based on OpenFOAM's `potentialFoam`. The reusable boilerplate lives in
`templates/potentialFoam/`; each scenario made from it gets a local workspace
under `workspaces/`. `simpleChannel` is the first scenario. `cylinder` and
`pitzDaily` are legacy inputs and are not part of the new workspace workflow.

The current Rust solver still runs from `corpus/simpleChannel` and builds a 2D
structured channel mesh from its `case.toml` when no mesh is supplied, or reads
an ASCII OpenFOAM `constant/polyMesh`. It solves for `Phi`, reconstructs `U`,
and writes `U`, `phi`, and `Phi`. The numerical path is limited to orthogonal
meshes and the boundary conditions used by the channel case. The new template
is the clean target format; wiring the solver to run directly from a workspace
is the next step.

### Mesh and case layout

The intended project layout separates reusable boilerplate from local runs:

```text
templates/potentialFoam/     versioned solver and scenario boilerplate
  channel/scenario.toml      starter geometry, mesh, and flow settings
workspaces/                  local scenarios and run artifacts (git-ignored)
reference/                   checked-in comparison data
corpus/                      legacy inputs retained during migration
```

New scenarios should start by copying a template into `workspaces/`. Keep
geometry inputs such as STEP files, generated meshes, settings, and outputs
inside that workspace. Only reusable defaults belong in `templates/`. The
template currently documents the intended format; it is not yet runnable
because the solver still expects the legacy OpenFOAM-style velocity field.

Launch the desktop STEP setup app with:

```sh
./openflowsim-gui.sh
```

The app opens to a welcome screen with recent workspaces and a **New
Workspace** flow. Choose an empty folder to use it directly as the workspace,
or choose `./workspaces/` to enter a name and create a workspace subfolder.
Nonempty folders outside the default workspace root are rejected. In the setup
window, import a STEP scene using the
copied Steposcope parser and geometry builder, then use the VTK viewport to
orbit, pan, zoom, and select geometry. Choose CAD Shaded, Clay, Glass,
Technical, or Face Orientation from the material selector to change how the
shape is displayed. Select a face and assign a boundary
with Enter, or use I/O/W for inlet/outlet/wall. Right-click an aperture edge
to detect its closed planar STEP edge loop and preview a translucent virtual
cap; assign the cap as inlet or outlet. Use **1 / 2 / 3 / 4** to switch
vertex, edge, face, or solid selection modes; the active mode is shown in the
bottom status area. Press **F** to focus and frame the selected component. The
app uses screen-space proximity to select the edge closest to the cursor.
Shift-click edges to build a multi-edge selection; matching a closed planar
STEP loop creates a virtual opening cap for inlet/outlet assignment. The app
auto-fits an editable domain
box around the scene. **Save Workspace** writes a `workspace.ofs` file and a
200 x 100 PNG preview in the workspace folder; the welcome screen lists saved
workspaces and opens them again. Vertical mouse movement while orbiting is
inverted. Importing geometry, changing domain bounds, or assigning a boundary
marks the workspace as modified; closing it offers Save, Discard, or Cancel.
Assigned boundaries appear as individual rows: click a row to focus its face or
opening edge, or click **×** to remove that assignment.

### Planned STEP-to-OFS workflow

The first complete product milestone is converting a FreeCAD STEP scene into
an OpenFlowSim `.ofs` setup. OpenFlowSim will remain a standalone desktop app
and will house its own Rust potential-flow solver.

1. Build a scene in FreeCAD and export it as STEP (AP203).
2. Open the STEP file in the OpenFlowSim desktop app.
3. Automatically fit a flow-domain bounding box around the imported scene.
4. Select geometry faces in the viewer and assign named boundary roles. Face
   selection should be available through a context menu and/or keyboard
   shortcut.
5. Save a sibling `.ofs` file next to the STEP file. The `.ofs` references the
   STEP path and stores its SHA-256 hash, the domain bounds, named face
   assignments, `.ofs` schema version, and OpenFlowSim writer version.
6. If the STEP file's hash changes, mark its sibling `.ofs` as invalid. The
   user must reopen the geometry and assign faces again before using the
   project.

The first milestone ends when STEP import, domain fitting, face assignment,
and `.ofs` save/load work. Next, connect the project to an external mesher and
then to the Rust solver. The external mesher is an explicit design choice; the
specific mesher and its mesh format remain open until solver mesh requirements
are clearer. The Rust solver will likely need further development to consume
those meshes and boundary assignments.

### Next steps

1. Define and implement the standalone STEP-to-OFS desktop workflow above.
2. Select an external mesher and define how `.ofs` named boundaries map to its
   mesh regions.
3. Extend the Rust solver to consume the selected mesh and boundary data.
4. Add another scenario only when the same solver workflow can run and validate
   it.

## Repository layout

| Path | Purpose |
| --- | --- |
| `templates/potentialFoam/` | Reusable scenario boilerplate for the Rust solver |
| `workspaces/` | Local, Git-ignored scenario inputs, meshes, and run artifacts |
| `tools/ofs_gui.py` | Python STEP viewer and boundary setup desktop app |
| `tools/step_viewer/vendor/step_explorer/` | Copied STEP parser, tessellator, and VTK viewer modules |
| `corpus/` | Legacy OpenFOAM case inputs retained during migration |
| `potentialFoam/` | OpenFOAM source used as the implementation reference |
| `reference/` | Stored OpenFOAM output fields and provenance |
| `results/` | Location for future Rust solver output (ignored by Git) |
| `solver/` | Rust `potentialFoam` implementation (in progress) |
| `validation/` | Rust validation crate, tolerances, tests, and documentation |

## Build and validate

Check that the Rust toolchain, Python, uv, and the project `.venv` are ready:

```sh
./checkreqs.sh
```

When requirements are missing, the script prints setup commands for Ubuntu.

From the repository root, run the validation tests:

```sh
cargo test
```

Run the Rust potential solver for the supported channel case:

```sh
cargo run -p foamcore-solver -- run corpus/simpleChannel
```

The command writes fields to `results/simpleChannel/1/`. It reads an existing
ASCII `constant/polyMesh` when present; otherwise, it builds the current
structured channel mesh from `case.toml`. The solver currently supports
orthogonal meshes and the case's fixed-velocity, zero-gradient, symmetry, and
empty boundary conditions.

Compare Rust output in `results/<case>/` with stored OpenFOAM fields:

```sh
cargo run --bin foamcore-validate -- compare simpleChannel
cargo run --bin foamcore-validate -- compare-all
```

Run the analytical uniform-channel check:

```sh
cargo run --bin foamcore-validate -- analytical simpleChannel
```

Run the full supported corpus pipeline:

```sh
./validate_corpus.sh
```

Inspect corpus metadata and data availability without running anything:

```sh
./corpus_overview.sh
```

The table shows whether a generated `polyMesh` exists, which mesh-generation
inputs are present, available initial/reference fields, and missing inputs.

Use `--run-dir DIR` to select a new output directory or `--no-html` to skip
the optional HTML rendering. Each run includes solver/comparison logs,
per-case JSON and Markdown, and aggregate `reports/corpus-validation.json` and
`reports/corpus-validation.md` files.

Render an existing run again with:

```sh
./genreport.sh --reports-dir validation/runs/<run-id>/reports \
  --output validation/runs/<run-id>/report.html
```

Reports are written under `validation/reports/`. The comparison command expects
solver results to be placed in `results/<case>/`; without them, the case is
reported as an error. See [`validation/README.md`](validation/README.md) for
details on the harness and its current limitations.

After generating validation JSON reports, create a browsable HTML summary with:

```sh
./genreport.sh
```

The page is written to `validation/reports/report.html` by default. An alternate
output path can be passed as the first argument. The HTML is self-contained and
can be opened in a browser or printed to PDF.

To regenerate the simple-channel OpenFOAM reference, install/source an
OpenFOAM 14 environment and run:

```sh
cargo run --bin foamcore-validate -- generate-reference simpleChannel
```

Reference generation protects existing data. Use the command's `--force`
option only when intentionally replacing a reference.

## Testing and validation roadmap

Treat verification and validation as related but distinct tasks. **Verification**
asks whether the code solves the intended discrete equations correctly.
**Validation** asks whether those equations and modeling choices describe
physical behavior, using suitable experimental or benchmark data. OpenFOAM is a
useful implementation reference, but agreement with it alone does not establish
physical correctness.

Build confidence in stages:

1. Test numerical building blocks against known answers with tight tolerances.
2. For `potentialFoam`, use small deterministic cases and analytical results
   where available. Compare fields with stated tolerances, and check flux or
   mass conservation and solver convergence.
3. For steady laminar cases, compare field norms alongside quantities such as
   velocity profiles and pressure drop. Add mesh refinement checks to see
   whether results converge as the mesh is refined.
4. For unsteady flows, compare early-time fields where useful, then compare
   time histories, shedding frequencies, forces, and conservation measures.
5. For chaotic or turbulent flows, do not require long-time cell-by-cell
   agreement. Compare statistical quantities, such as mean fields and energy
   spectra, over documented sampling windows and uncertainty ranges.

Use visualizations to spot artifacts and implausible flow patterns. They are a
diagnostic aid alongside numerical checks, not a substitute for them. Start
with the existing `simpleChannel` analytical check and OpenFOAM reference;
add benchmark cases and references as each new validation level is implemented.

## Solver behavior to port

The main OpenFOAM flow in `potentialFoam/potentialFoam.C` is:

1. Read `U`, initialize it to zero, and form the face flux `phi`.
2. Infer pressure and velocity-potential boundary conditions from the input
   fields.
3. Apply rotating-frame and boundary-flux adjustments.
4. Solve the `Phi` Laplace equation, correcting `phi` with the equation flux.
5. Reconstruct `U` from the corrected flux and correct its boundary values.
6. Optionally calculate and write pressure; write the resulting fields.

The Rust implementation should be developed against this flow and validated
incrementally with the stored reference and analytical checks. The simple
channel provides the first end-to-end case; additional references are needed
before the other corpus cases can serve as regression tests.
