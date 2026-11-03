#!/usr/bin/env python3
"""Build one machine-readable and Markdown report for a corpus run."""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path


def read_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def read_code(path):
    try:
        return int(path.read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return None


def case_result(case_dir, run_dir, ref_dir):
    name = case_dir.name
    state_file = run_dir / "status" / f"{name}.txt"
    state_lines = state_file.read_text(encoding="utf-8").splitlines() if state_file.exists() else []
    state = state_lines[0] if state_lines else "RUN"
    reason = state_lines[1] if len(state_lines) > 1 else None
    comparison_path = run_dir / "reports" / f"{name}.comparison.json"
    analytical_path = run_dir / "reports" / f"{name}.analytical.json"
    comparison = read_json(comparison_path)
    analytical = read_json(analytical_path)
    case_comparison = None
    if isinstance(comparison, dict):
        reports = comparison.get("cases", [])
        case_comparison = reports[0] if reports else None

    solver_exit = read_code(run_dir / "status" / f"{name}.solver.exit")
    compare_exit = read_code(run_dir / "status" / f"{name}.compare.exit")
    analytical_exit = read_code(run_dir / "status" / f"{name}.analytical.exit")

    if state == "SKIP":
        status, detail = "SKIP", reason or "Case is not currently runnable."
    elif solver_exit is None:
        status, detail = "ERROR", "Solver did not produce an exit status."
    elif solver_exit != 0:
        status, detail = "ERROR", f"Solver exited with status {solver_exit}."
    elif compare_exit is None or case_comparison is None:
        status, detail = "ERROR", "Reference comparison did not produce a report."
    elif case_comparison.get("overall_status") == "Error":
        status, detail = "ERROR", "Reference comparison could not complete; inspect the comparison report and log."
    elif compare_exit != 0 or case_comparison.get("overall_status") == "Fail":
        status, detail = "FAIL", "One or more reference comparisons failed."
    elif analytical_exit is not None and analytical is None:
        status, detail = "ERROR", "Analytical check did not produce a report."
    elif analytical is not None and not analytical.get("all_passed", False):
        status, detail = "FAIL", "The analytical check failed."
    else:
        status, detail = "PASS", "Solver run and applicable validation checks passed."

    command = f"cargo run -p foamcore-solver -- run {case_dir} --output {run_dir / 'results' / name}"
    return {
        "case_name": name,
        "status": status,
        "overall_status": status.title(),
        "summary": detail,
        "solver": {
            "command": command,
            "exit_code": solver_exit,
            "log": f"logs/{name}.solver.log",
            "output_dir": f"results/{name}",
        },
        "reference": {
            "path": str(ref_dir / name),
            "available": (ref_dir / name / "U").is_file() and (ref_dir / name / "phi").is_file(),
        },
        "comparison_exit_code": compare_exit,
        "comparison": comparison,
        "comparisons": case_comparison.get("comparisons", []) if case_comparison else [],
        "analytical_exit_code": analytical_exit,
        "analytical": analytical,
        "detail": detail,
    }


def markdown(report):
    summary = report["summary"]
    lines = [
        "# Corpus validation run",
        "",
        f"- Run: `{report['run_id']}`",
        f"- Generated: {report['generated_at']}",
        f"- Overall: **{report['status']}**",
        f"- Cases: {summary['passed']} passed, {summary['failed']} failed, "
        f"{summary['errors']} errors, {summary['skipped']} skipped",
        "",
    ]
    for case in report["cases"]:
        lines.extend([f"## {case['case_name']} — {case['status']}", "", case["detail"], ""])
        lines.append(f"Solver: `{case['solver']['command']}` (exit {case['solver']['exit_code']})")
        lines.append(f"Solver log: `{case['solver']['log']}`")
        lines.append(f"Output: `{case['solver']['output_dir']}`")
        if case.get("comparisons"):
            lines.extend(["", "### Reference comparisons", "", "| Field | Status | Values | Max abs error | RMSE | Relative L2 |", "|---|---:|---:|---:|---:|---:|"])
            for entry in case["comparisons"]:
                values = entry.get("n_values", "")
                max_abs = entry.get("max_abs_error", "")
                rmse = entry.get("rmse", "")
                rel_l2 = entry.get("rel_l2_error", "")
                lines.append(f"| {entry.get('field_name', '')} | {entry.get('status', '')} | {values} | {max_abs} | {rmse} | {rel_l2} |")
        elif case["status"] == "SKIP":
            lines.extend(["", f"Reference comparison skipped: {case['detail']}"])
        analytical = case.get("analytical")
        if analytical:
            lines.extend(["", "### Analytical check", "", analytical.get("description", "")])
            for key, label in (("u_comparison", "U"), ("phi_comparison", "Phi")):
                comparison = analytical.get(key)
                if comparison:
                    lines.append(f"- {label}: {'PASS' if comparison.get('passed') else 'FAIL'}, "
                                 f"max abs error {comparison.get('max_abs_error')}, "
                                 f"RMSE {comparison.get('rmse')}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--corpus-dir", type=Path, required=True)
    parser.add_argument("--reference-dir", type=Path, required=True)
    args = parser.parse_args()

    cases = [case_result(path, args.run_dir, args.reference_dir)
             for path in sorted(args.corpus_dir.iterdir())
             if path.is_dir() and (path / "case.toml").is_file()]
    counts = {key: sum(case["status"] == value for case in cases)
              for key, value in (("passed", "PASS"), ("failed", "FAIL"),
                                 ("errors", "ERROR"), ("skipped", "SKIP"))}
    executed = counts["passed"] + counts["failed"] + counts["errors"]
    all_passed = executed > 0 and counts["failed"] == 0 and counts["errors"] == 0
    status = "PASS" if all_passed else "FAIL" if executed else "SKIP"
    report = {
        "schema_version": 1,
        "run_id": args.run_dir.name,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "status": status,
        "all_passed": all_passed,
        "summary": {"total": len(cases), "executed": executed, **counts},
        "cases": cases,
    }
    report_path = args.run_dir / "reports" / "corpus-validation.json"
    markdown_path = report_path.with_suffix(".md")
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(markdown(report), encoding="utf-8")
    print(f"Corpus validation: {status} ({counts['passed']} passed, {counts['failed']} failed, "
          f"{counts['errors']} errors, {counts['skipped']} skipped)")
    print(f"JSON: {report_path}")
    print(f"Markdown: {markdown_path}")
    if status == "PASS":
        return 0
    return 2 if status == "SKIP" else 1


if __name__ == "__main__":
    raise SystemExit(main())
