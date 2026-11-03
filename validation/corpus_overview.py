#!/usr/bin/env python3
"""Print case metadata and mesh/field availability as a terminal table."""

import argparse
import shutil
import tomllib
from pathlib import Path


def case_info(case_dir, reference_dir):
    metadata_path = case_dir / "case.toml"
    try:
        metadata = tomllib.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        metadata = {}
        metadata_error = f"case.toml unreadable: {exc}"
    else:
        metadata_error = None

    case = metadata.get("case", {})
    geometry = metadata.get("geometry", {})
    mesh_dir = case_dir / "constant" / "polyMesh"
    generated_mesh = mesh_dir.is_dir() and all(
        (mesh_dir / name).is_file() for name in ("points", "faces", "owner", "neighbour", "boundary")
    )
    mesh_inputs = []
    if generated_mesh:
        mesh_inputs.append("constant/polyMesh")
    if (case_dir / "system" / "blockMeshDict").is_file():
        mesh_inputs.append("blockMeshDict")
    if (case_dir / "Allrun").is_file():
        mesh_inputs.append("Allrun")
    if geometry.get("cells_per_axis"):
        mesh_inputs.append("case.toml cell counts")

    initial_fields = []
    missing_initial = []
    for field in ("U", "p"):
        candidates = [path for path in (case_dir / "0" / field, case_dir / "0" / f"{field}.orig") if path.is_file()]
        if candidates:
            initial_fields.append(candidates[0].name)
        elif field == "U":
            missing_initial.append("U")

    ref_case = reference_dir / case_dir.name
    reference_fields = [field for field in ("U", "phi", "Phi") if (ref_case / field).is_file()]
    missing_reference = [field for field in ("U", "phi") if field not in reference_fields]

    notes = []
    if not generated_mesh:
        notes.append("no generated polyMesh")
    if not mesh_inputs:
        notes.append("no mesh-generation inputs found")
    if missing_initial:
        notes.append("missing initial " + ", ".join(missing_initial))
    if missing_reference:
        notes.append("missing reference " + ", ".join(missing_reference))
    if metadata_error:
        notes.append(metadata_error)
    if not notes:
        notes.append("inputs and U/phi references present")

    return {
        "name": case_dir.name,
        "solver": case.get("solver", "—"),
        "type": case.get("type", "—"),
        "description": case.get("description", "—"),
        "generated_mesh": "yes" if generated_mesh else "no",
        "mesh_inputs": ", ".join(mesh_inputs) or "none",
        "initial_fields": ", ".join(initial_fields) or "none",
        "reference_fields": ", ".join(reference_fields) or "none",
        "notes": "; ".join(notes),
    }


def print_table(rows):
    columns = [
        ("CASE", "name"),
        ("SOLVER", "solver"),
        ("TYPE", "type"),
        ("GENERATED MESH", "generated_mesh"),
        ("MESH INPUTS", "mesh_inputs"),
        ("INITIAL FIELDS", "initial_fields"),
        ("REFERENCE FIELDS", "reference_fields"),
        ("NOTES", "notes"),
    ]
    widths = [max(len(title), *(len(str(row[key])) for row in rows)) for title, key in columns]
    total = sum(widths) + 3 * (len(widths) - 1)
    terminal_width = shutil.get_terminal_size((160, 24)).columns
    if total > terminal_width:
        # Keep availability columns readable; wrap descriptions/notes as a
        # separate block below each compact table row.
        columns = columns[:-1]
        widths = [max(len(title), *(len(str(row[key])) for row in rows)) for title, key in columns]
    row_format = "  ".join(f"{{:<{width}}}" for width in widths)
    print(row_format.format(*(title for title, _ in columns)))
    print(row_format.format(*("-" * width for width in widths)))
    for row in rows:
        print(row_format.format(*(str(row[key]) for _, key in columns)))
        if len(columns) != 8:
            print(f"  NOTES: {row['notes']}")
        print(f"  DESCRIPTION: {row['description']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus-dir", type=Path, required=True)
    parser.add_argument("--reference-dir", type=Path, required=True)
    args = parser.parse_args()

    rows = [case_info(path, args.reference_dir)
            for path in sorted(args.corpus_dir.iterdir())
            if path.is_dir() and (path / "case.toml").is_file()]
    if not rows:
        print(f"No corpus cases with case.toml found in {args.corpus_dir}")
        return 0
    print_table(rows)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
