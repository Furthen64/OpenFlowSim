#!/usr/bin/env python3
"""Render validation JSON artifacts as a self-contained HTML report."""

import argparse
import html
import json
from pathlib import Path


def fmt(value, precision=6):
    if value is None:
        return "—"
    try:
        return f"{float(value):.{precision}e}"
    except (TypeError, ValueError):
        return str(value)


def read_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def status(value):
    value = str(value or "UNKNOWN").upper()
    css = value.lower() if value in {"PASS", "FAIL", "ERROR", "SKIP"} else "unknown"
    return f'<span class="status {css}">{html.escape(value)}</span>'


def comparison_row(case_name, field_name, entry):
    # CaseReport entries flatten ComparisonResult fields into each entry;
    # analytical reports keep each result nested under its field key.
    result = entry.get("result") or entry
    tol = result.get("tolerance") or {}
    details = entry.get("error_message") or entry.get("skip_reason") or ""
    components = result.get("max_component_errors")
    if components:
        details = (details + " · " if details else "") + "component max: " + ", ".join(fmt(v) for v in components)
    cells = [
        f"<td>{html.escape(str(case_name))}</td>",
        f"<td><strong>{html.escape(str(field_name))}</strong></td>",
        f"<td>{status(entry.get('status', 'PASS' if result.get('passed') else 'ERROR'))}</td>",
        f"<td>{html.escape(str(result.get('n_values', '—')))}</td>",
        f"<td>{fmt(result.get('max_abs_error'))}</td>",
        f"<td>{fmt(result.get('rmse'))}</td>",
        f"<td>{fmt(result.get('rel_l2_error'))}</td>",
        f"<td>{fmt(tol.get('atol'))} / {fmt(tol.get('rtol'))}</td>",
        f"<td class=\"details\">{html.escape(str(details))}</td>",
    ]
    return "<tr>\n" + "\n".join(cells) + "\n</tr>"


def render(reports_dir):
    reports = sorted(reports_dir.glob("*.json"))
    case_reports_by_name = {}
    analytical = []
    for path in reports:
        data = read_json(path)
        if not isinstance(data, dict):
            continue
        if "comparisons" in data:
            case_reports_by_name[data.get("case_name", path.stem)] = (path, data)
        elif "cases" in data and isinstance(data["cases"], list):
            for case in data["cases"]:
                case_reports_by_name[case.get("case_name", path.stem)] = (path, case)
        elif "u_comparison" in data or "phi_comparison" in data:
            analytical.append((path, data))

    rows = []
    case_cards = []
    case_reports = list(case_reports_by_name.values())
    for path, case in case_reports:
        case_name = case.get("case_name", path.stem)
        entries = case.get("comparisons") or []
        passed = sum(1 for e in entries if e.get("status") == "Pass")
        overall = case.get("overall_status", "UNKNOWN")
        case_cards.append(
            f'<div class="case-card"><div>{html.escape(str(case_name))}</div>'
            f'<div>{status(overall)}</div><small>{passed}/{len(entries)} fields passed</small></div>'
        )
        rows.extend(comparison_row(case_name, e.get("field_name", "field"), e) for e in entries)

    analytical_rows = []
    for path, data in analytical:
        case_name = data.get("case_name", path.stem)
        for key, field_name in (("u_comparison", "U (analytical)"), ("phi_comparison", "Phi (analytical)")):
            result = data.get(key)
            if result:
                entry = {
                    "status": "Pass" if result.get("passed") else "Fail",
                    "result": result,
                }
                analytical_rows.append(comparison_row(case_name, field_name, entry))
        if data.get("phi_missing"):
            analytical_rows.append(comparison_row(case_name, "Phi (analytical)", {
                "status": "Skip", "skip_reason": "Phi field not available"
            }))

    total = len(case_reports)
    passing = sum(1 for _, case in case_reports if case.get("overall_status") == "Pass")
    headline = "Validation reports" if reports else "No validation reports found"
    if not reports:
        case_cards.append('<p class="empty">Run a validation command to create JSON reports, then rerun <code>./genreport.sh</code>.</p>')
    generated = max((p.stat().st_mtime for p in reports), default=None)
    generated_text = "JSON artifacts found: " + str(len(reports))
    if generated is not None:
        from datetime import datetime
        generated_text += " · Latest file: " + datetime.fromtimestamp(generated).astimezone().strftime("%Y-%m-%d %H:%M %Z")

    def table(body):
        if not body:
            return '<p class="empty">No data in this section yet.</p>'
        return '''<div class="table-wrap">
<table>
<thead>
<tr>
<th>Case</th>
<th>Field</th>
<th>Status</th>
<th>Values</th>
<th>Max abs error</th>
<th>RMSE</th>
<th>Relative L2</th>
<th>Atol / Rtol</th>
<th>Notes</th>
</tr>
</thead>
<tbody>
''' + "\n".join(body) + '''
</tbody>
</table>
</div>'''

    summary = f"{passing} of {total} case reports pass" if total else "No case comparison reports yet"
    case_cards_html = "\n".join(case_cards)
    return f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>OpenFlowSim validation report</title>
<style>
:root{{
  --ink:#e4eaf0;
  --muted:#aab8c6;
  --line:#354657;
  --paper:#202f3d;
  --bg:#17232e;
  --blue:#8bbbe0;
  --header:#293b4b;
}}
*{{box-sizing:border-box}}
body{{
  margin:0;
  background:var(--bg);
  color:var(--ink);
  font:15px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;
}}
main{{max-width:1440px;margin:0 auto;padding:36px 24px 64px}}
header{{margin-bottom:26px}}
h1{{font-size:30px;margin:0 0 5px}}
h2{{font-size:20px;margin:34px 0 12px}}
p{{margin:5px 0;color:var(--muted)}}
.overview{{display:flex;gap:12px;flex-wrap:wrap;margin:20px 0}}
.case-card{{
  background:var(--paper);
  border:1px solid var(--line);
  border-radius:10px;
  padding:15px 17px;
  min-width:190px;
  display:grid;
  gap:7px;
  font-weight:650;
}}
.case-card small{{font-weight:400;color:var(--muted)}}
.status{{
  display:inline-block;
  border-radius:999px;
  padding:2px 9px;
  font-size:12px;
  font-weight:750;
  letter-spacing:.04em;
}}
.pass{{background:#244b3d;color:#b9f0d0}}
.fail,.error{{background:#593532;color:#ffc4bc}}
.skip,.unknown{{background:#3a4652;color:#d0dae3}}
.table-wrap{{
  overflow:auto;
  border:1px solid var(--line);
  border-radius:10px;
  background:var(--paper);
}}
table{{
  border-collapse:collapse;
  width:100%;
  min-width:970px;
  font-variant-numeric:tabular-nums;
}}
th,td{{
  text-align:left;
  padding:10px 12px;
  border-bottom:1px solid var(--line);
  white-space:nowrap;
}}
th{{
  background:var(--header);
  color:#d4e0eb;
  font-size:12px;
  text-transform:uppercase;
  letter-spacing:.04em;
}}
tr:last-child td{{border-bottom:0}}
td.details{{white-space:normal;min-width:180px;color:var(--muted)}}
code{{background:#304353;color:#e4eaf0;border-radius:4px;padding:2px 5px}}
.empty{{padding:16px;background:var(--paper);border-radius:8px}}
footer{{margin-top:30px;color:var(--muted);font-size:13px}}
@media print{{
  :root{{
    --ink:#17212b;
    --muted:#617080;
    --line:#dce3e9;
    --paper:#fff;
    --bg:#fff;
    --header:#edf2f6;
  }}
  body{{background:#fff;color:var(--ink)}}
  main{{max-width:none;padding:10px}}
  .table-wrap{{overflow:visible}}
  table{{min-width:0;font-size:11px}}
  th,td{{padding:6px}}
}}
</style>
</head>
<body>
<main>
<header>
<h1>OpenFlowSim validation</h1>
<p>{html.escape(headline)} · {html.escape(summary)}</p>
<p>{html.escape(generated_text)}</p>
</header>
<h2>Case summary</h2>
<section class="overview">{case_cards_html}</section>
<h2>OpenFOAM field comparisons</h2>
{table(rows)}
<h2>Analytical checks</h2>
{table(analytical_rows)}
<footer>
Generated from JSON artifacts in <code>{html.escape(str(reports_dir))}</code>.
Re-run validation before generating this page to ensure it reflects the latest solver output.
</footer>
</main>
</body>
</html>
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reports-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(render(args.reports_dir), encoding="utf-8")


if __name__ == "__main__":
    main()
