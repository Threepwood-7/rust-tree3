use crate::tree::Tree;

const NODE_RADIUS: f64 = 20.0;
const LEVEL_HEIGHT: f64 = 80.0;
const H_SPACING: f64 = 50.0;
const PADDING: f64 = 40.0;

/// Color for each label (1-indexed)
fn label_color(label: u32) -> &'static str {
    match label {
        1 => "#4a90d9",
        2 => "#e8a838",
        3 => "#e84c4c",
        4 => "#6ab04c",
        5 => "#9b59b6",
        6 => "#1abc9c",
        _ => "#888888",
    }
}

struct Layout {
    x: Vec<f64>,
    y: Vec<f64>,
}

/// Assign positions to nodes using a simple recursive centering approach.
fn compute_layout(tree: &Tree) -> Layout {
    let n = tree.nodes.len();
    let mut x = vec![0.0f64; n];
    let mut y = vec![0.0f64; n];
    let mut next_leaf = 0.0f64;

    assign_positions(tree, tree.root, 0, &mut x, &mut y, &mut next_leaf);

    // Shift everything so min_x = PADDING
    let min_x = x.iter().cloned().fold(f64::INFINITY, f64::min);
    for xi in x.iter_mut() {
        *xi = *xi - min_x + PADDING;
    }

    Layout { x, y }
}

fn assign_positions(
    tree: &Tree,
    node: usize,
    depth: usize,
    x: &mut Vec<f64>,
    y: &mut Vec<f64>,
    next_leaf: &mut f64,
) {
    y[node] = PADDING + 30.0 + depth as f64 * LEVEL_HEIGHT;

    let children: Vec<usize> = tree.nodes[node].children.clone();

    if children.is_empty() {
        x[node] = *next_leaf;
        *next_leaf += H_SPACING * 2.0;
    } else {
        for &child in &children {
            assign_positions(tree, child, depth + 1, x, y, next_leaf);
        }
        let first = children[0];
        let last = children[children.len() - 1];
        x[node] = (x[first] + x[last]) / 2.0;
    }
}

/// Render a tree as an SVG string.
pub fn render_svg(tree: &Tree, title: &str) -> String {
    let layout = compute_layout(tree);

    let max_x = layout.x.iter().cloned().fold(0.0f64, f64::max) + PADDING;
    let max_y = layout.y.iter().cloned().fold(0.0f64, f64::max) + PADDING;

    let width = (max_x + NODE_RADIUS + PADDING).max(200.0);
    let height = max_y + NODE_RADIUS + PADDING;

    let mut lines: Vec<String> = Vec::new();

    lines.push(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#
    ));
    lines.push(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}">"#,
        w = width,
        h = height
    ));
    lines.push(format!(
        r#"  <rect width="100%" height="100%" fill="{bg}"/>"#,
        bg = "#f8f8f8"
    ));
    lines.push(format!(
        r#"  <text x="10" y="20" font-family="monospace" font-size="13" fill="{fg}">{t}</text>"#,
        fg = "#333333",
        t = escape_xml(title)
    ));

    // Draw edges first
    for node_idx in 0..tree.nodes.len() {
        let nx = layout.x[node_idx];
        let ny = layout.y[node_idx];
        for &child_idx in &tree.nodes[node_idx].children {
            let cx = layout.x[child_idx];
            let cy = layout.y[child_idx];
            lines.push(format!(
                r#"  <line x1="{nx:.1}" y1="{ny:.1}" x2="{cx:.1}" y2="{cy:.1}" stroke="{sc}" stroke-width="2"/>"#,
                nx = nx,
                ny = ny,
                cx = cx,
                cy = cy,
                sc = "#999999"
            ));
        }
    }

    // Draw nodes
    for node_idx in 0..tree.nodes.len() {
        let nx = layout.x[node_idx];
        let ny = layout.y[node_idx];
        let label = tree.nodes[node_idx].label;
        let color = label_color(label);
        let stroke = "#333333";
        let text_fill = "#ffffff";

        lines.push(format!(
            r#"  <circle cx="{nx:.1}" cy="{ny:.1}" r="{r}" fill="{color}" stroke="{stroke}" stroke-width="1.5"/>"#,
            nx = nx,
            ny = ny,
            r = NODE_RADIUS,
            color = color,
            stroke = stroke
        ));
        lines.push(format!(
            r#"  <text x="{nx:.1}" y="{ny:.1}" text-anchor="middle" dominant-baseline="middle" font-family="monospace" font-size="14" font-weight="bold" fill="{fill}">{label}</text>"#,
            nx = nx,
            ny = ny,
            fill = text_fill,
            label = label
        ));
    }

    lines.push("</svg>".to_string());
    lines.join("\n") + "\n"
}

/// Render a grid overview of all trees found so far.
///
/// Each entry is `(tree, sequence_index, canonical_form)`.
/// Called after every new acceptance; the file is rewritten in place.
pub fn render_overview_svg(entries: &[(&Tree, usize, &str)]) -> String {
    let n = entries.len();
    if n == 0 {
        return String::new();
    }

    // Colors (kept as variables so they never appear inline inside r#"..."# literals,
    // which would terminate the raw string at the embedded `"#` sequence).
    let bg_page   = "#1a1a2e";
    let bg_cell   = "#252540";
    let stroke_cell = "#3a3a5c";
    let fg_title  = "#e0e0ff";
    let fg_canon  = "#8888aa";
    let edge_col  = "#555577";
    let node_stroke = "#222244";
    let text_fill = "#ffffff";

    let cols: usize = 5;
    let cell_w: f64 = 210.0;
    let cell_h: f64 = 230.0;
    let label_h: f64 = 38.0;
    let gap: f64 = 14.0;
    let padding: f64 = 20.0;

    let rows = (n + cols - 1) / cols;
    let total_w = cols as f64 * (cell_w + gap) - gap + 2.0 * padding;
    let total_h = rows as f64 * (cell_h + label_h + gap) - gap + 2.0 * padding;

    let mut out: Vec<String> = Vec::new();
    out.push(r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string());
    out.push(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}">"#,
        w = total_w, h = total_h
    ));
    out.push(format!(r#"  <rect width="100%" height="100%" fill="{bg}"/>"#, bg = bg_page));

    for (i, (tree, index, canonical)) in entries.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let ox = padding + col as f64 * (cell_w + gap);
        let oy = padding + row as f64 * (cell_h + label_h + gap);

        // Cell background
        out.push(format!(
            r#"  <rect x="{ox:.0}" y="{oy:.0}" width="{cw:.0}" height="{ch:.0}" fill="{bg}" rx="8" stroke="{sc}" stroke-width="1.5"/>"#,
            ox = ox, oy = oy, cw = cell_w, ch = cell_h + label_h,
            bg = bg_cell, sc = stroke_cell
        ));

        // Header: "T1 · 3 nodes"
        out.push(format!(
            r#"  <text x="{tx:.0}" y="{ty:.0}" text-anchor="middle" font-family="monospace" font-size="13" font-weight="bold" fill="{fg}">T{idx} · {nodes} nodes</text>"#,
            tx = ox + cell_w / 2.0, ty = oy + 15.0,
            fg = fg_title, idx = index, nodes = tree.nodes.len()
        ));

        // Canonical form (truncated if long)
        let canon_show = if canonical.len() > 30 {
            format!("{}…", &canonical[..27])
        } else {
            canonical.to_string()
        };
        out.push(format!(
            r#"  <text x="{tx:.0}" y="{ty:.0}" text-anchor="middle" font-family="monospace" font-size="10" fill="{fg}">{c}</text>"#,
            tx = ox + cell_w / 2.0, ty = oy + 30.0,
            fg = fg_canon, c = escape_xml(&canon_show)
        ));

        // Nested SVG: scales the tree to fill the cell, preserving aspect ratio.
        let layout = compute_layout(tree);
        let nat_w = layout.x.iter().cloned().fold(0.0f64, f64::max)
            + PADDING + NODE_RADIUS + PADDING;
        let nat_h = layout.y.iter().cloned().fold(0.0f64, f64::max)
            + NODE_RADIUS + PADDING;

        out.push(format!(
            r#"  <svg x="{sx:.0}" y="{sy:.0}" width="{sw:.0}" height="{sh:.0}" viewBox="0 0 {vw:.0} {vh:.0}" preserveAspectRatio="xMidYMid meet" overflow="hidden">"#,
            sx = ox + 5.0, sy = oy + label_h,
            sw = cell_w - 10.0, sh = cell_h - 5.0,
            vw = nat_w, vh = nat_h,
        ));

        // Edges
        for node_idx in 0..tree.nodes.len() {
            for &child_idx in &tree.nodes[node_idx].children {
                out.push(format!(
                    r#"    <line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{sc}" stroke-width="2"/>"#,
                    x1 = layout.x[node_idx], y1 = layout.y[node_idx],
                    x2 = layout.x[child_idx], y2 = layout.y[child_idx],
                    sc = edge_col,
                ));
            }
        }

        // Nodes
        for node_idx in 0..tree.nodes.len() {
            let nx = layout.x[node_idx];
            let ny = layout.y[node_idx];
            let label = tree.nodes[node_idx].label;
            let color = label_color(label);
            out.push(format!(
                r#"    <circle cx="{nx:.1}" cy="{ny:.1}" r="{r}" fill="{color}" stroke="{sc}" stroke-width="1.5"/>"#,
                nx = nx, ny = ny, r = NODE_RADIUS, color = color, sc = node_stroke
            ));
            out.push(format!(
                r#"    <text x="{nx:.1}" y="{ny:.1}" text-anchor="middle" dominant-baseline="middle" font-family="monospace" font-size="14" font-weight="bold" fill="{fill}">{label}</text>"#,
                nx = nx, ny = ny, fill = text_fill, label = label
            ));
        }

        out.push("  </svg>".to_string());
    }

    out.push("</svg>".to_string());
    out.join("\n") + "\n"
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
