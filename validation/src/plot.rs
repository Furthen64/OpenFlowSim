//! Lightweight SVG field plots for cell-centred OpenFOAM results.

use crate::field::{Field, FieldType};
use foamcore_solver::mesh::MeshGeometry;
use std::path::Path;

/// Write a three-panel SVG showing reference, computed, and absolute error.
/// Cells are drawn at their mesh centres, so the same plot works for structured
/// and unstructured meshes without external plotting dependencies.
pub fn write_field_plot(
    path: &Path,
    reference: &Field,
    computed: &Field,
    geometry: &MeshGeometry,
) -> Result<(), String> {
    if reference.n_values != geometry.cells.len() || computed.n_values != geometry.cells.len() {
        return Err("field and mesh cell counts do not match".into());
    }
    if reference.field_type != computed.field_type {
        return Err("reference and computed field types do not match".into());
    }
    let components = match reference.field_type {
        FieldType::Scalar => vec![("value", reference.values.clone(), computed.values.clone())],
        FieldType::Vector => {
            let magnitude = |f: &Field| {
                (0..f.n_values)
                    .map(|i| {
                        let y = f.extra_components.first().map_or(0.0, |v| v[i]);
                        let z = f.extra_components.get(1).map_or(0.0, |v| v[i]);
                        (f.values[i] * f.values[i] + y * y + z * z).sqrt()
                    })
                    .collect::<Vec<_>>()
            };
            vec![("magnitude", magnitude(reference), magnitude(computed))]
        }
    };
    let (label, reference_values, computed_values) = &components[0];
    let errors: Vec<f64> = reference_values
        .iter()
        .zip(computed_values)
        .map(|(a, b)| (a - b).abs())
        .collect();
    let all_x: Vec<f64> = geometry.cells.iter().map(|c| c.centre.x).collect();
    let all_y: Vec<f64> = geometry.cells.iter().map(|c| c.centre.y).collect();
    let min_x = all_x.iter().copied().fold(f64::INFINITY, f64::min);
    let max_x = all_x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min_y = all_y.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = all_y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let width = 1080.0;
    let height = 430.0;
    let panel_w = 340.0;
    let panel_h = 330.0;
    let margin = 10.0;
    let plot_margin = 30.0;
    let scale = ((panel_w - 2.0 * plot_margin) / (max_x - min_x).max(1e-15))
        .min((panel_h - 2.0 * plot_margin) / (max_y - min_y).max(1e-15));
    let plot_w = (max_x - min_x) * scale;
    let plot_h = (max_y - min_y) * scale;
    let radius = (scale * 0.45).clamp(0.6, 5.0);
    let ranges = [
        value_range(reference_values, computed_values),
        value_range(reference_values, computed_values),
        (0.0, errors.iter().copied().fold(0.0_f64, f64::max)),
    ];
    let datasets = [reference_values, computed_values, &errors];
    let titles = ["OpenFOAM reference", "Rust result", "Absolute error"];
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n<style>text{{font-family: sans-serif;fill:#202124}} .title{{font-size:16px;font-weight:600}} .axis{{font-size:11px}}</style>\n<text x=\"10\" y=\"22\" class=\"title\">{} — {label}</text>\n",
        escape_xml(&reference.name)
    );
    for panel in 0..3 {
        let ox = margin + panel as f64 * (panel_w + 15.0);
        let oy = 42.0;
        let px = ox + (panel_w - plot_w) * 0.5;
        let py = oy + plot_margin;
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" class=\"title\">{}</text>\n<rect x=\"{px:.2}\" y=\"{py:.2}\" width=\"{plot_w:.2}\" height=\"{plot_h:.2}\" fill=\"#f7f7f7\" stroke=\"#555\" stroke-width=\"0.5\"/>\n",
            ox + 4.0,
            oy + 17.0,
            titles[panel]
        ));
        let (lo, hi) = ranges[panel];
        for (i, cell) in geometry.cells.iter().enumerate() {
            let x = px + (cell.centre.x - min_x) * scale;
            let y = py + plot_h - (cell.centre.y - min_y) * scale;
            let color = color_map(datasets[panel][i], lo, hi);
            svg.push_str(&format!(
                "<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"{radius:.2}\" fill=\"{color}\"/>\n"
            ));
        }
        svg.push_str(&format!(
            "<text x=\"{px:.2}\" y=\"{}\" class=\"axis\">x</text><text x=\"{}\" y=\"{py:.2}\" class=\"axis\">y</text><text x=\"{px:.2}\" y=\"{}\" class=\"axis\">{lo:.3e}</text><text x=\"{}\" y=\"{}\" class=\"axis\">{hi:.3e}</text>\n",
            py + plot_h + 18.0,
            px - 20.0,
            py + plot_h + 18.0,
            px + plot_w - 45.0,
            py + plot_h + 18.0
        ));
    }
    svg.push_str("</svg>\n");
    std::fs::write(path, svg).map_err(|e| e.to_string())
}

fn value_range(a: &[f64], b: &[f64]) -> (f64, f64) {
    let lo = a.iter().chain(b).copied().fold(f64::INFINITY, f64::min);
    let hi = a.iter().chain(b).copied().fold(f64::NEG_INFINITY, f64::max);
    if (hi - lo).abs() < f64::EPSILON {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    }
}

fn color_map(value: f64, lo: f64, hi: f64) -> String {
    let t = ((value - lo) / (hi - lo).abs().max(f64::EPSILON)).clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.5 {
        let q = t * 2.0;
        (
            (40.0 + 210.0 * q) as u8,
            (90.0 + 150.0 * q) as u8,
            (190.0 - 150.0 * q) as u8,
        )
    } else {
        let q = (t - 0.5) * 2.0;
        (250, (240.0 - 190.0 * q) as u8, (40.0 - 25.0 * q) as u8)
    };
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_plot_contains_three_panels() {
        let geometry = MeshGeometry {
            faces: vec![],
            cells: vec![
                foamcore_solver::mesh::CellGeometry {
                    centre: foamcore_solver::mesh::Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    volume: 1.0,
                },
                foamcore_solver::mesh::CellGeometry {
                    centre: foamcore_solver::mesh::Vector3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    volume: 1.0,
                },
            ],
        };
        let reference = Field::scalar("p", vec![0.0, 1.0]);
        let computed = Field::scalar("p", vec![0.1, 0.9]);
        let path = std::env::temp_dir().join("foamcore-test-plot.svg");
        write_field_plot(&path, &reference, &computed, &geometry).unwrap();
        let svg = std::fs::read_to_string(&path).unwrap();
        assert_eq!(svg.matches("<circle").count(), 6);
        let _ = std::fs::remove_file(path);
    }
}
