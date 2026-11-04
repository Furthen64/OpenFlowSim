# potentialFoam workspace template

This template is the reusable starting point for scenarios solved with the
Rust `foamcore-solver`. Copy a scenario into a local workspace, then adapt its
geometry, mesh resolution, and flow conditions there.

```text
templates/potentialFoam/       versioned boilerplate
workspaces/<name>/             local scenarios and experiments (ignored by Git)
```

The `channel/` scenario is a clean, solver-owned configuration example. It
does not use OpenFOAM `0/`, `system/`, or `U.orig` files. The solver currently
still reads an OpenFOAM-style velocity field, so this template describes the
target workspace shape and is not yet a directly runnable solver case. The
next implementation step is teaching the solver to read this configuration
and initialize velocity from it.

Create a working copy with:

```sh
mkdir -p workspaces/channel
cp templates/potentialFoam/channel/scenario.toml workspaces/channel/scenario.toml
```

Keep generated meshes, solver outputs, and experiments inside the workspace.
Only reusable boilerplate belongs under `templates/`.
