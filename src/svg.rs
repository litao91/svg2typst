use std::str::FromStr;

use log::debug;
use svgtypes::Transform;

use crate::dom::Node;
use crate::emit::Emitter;
use crate::geom::{apply, compose, is_axis_aligned, is_similarity, scale_mag};
use crate::path;
use crate::path::Pt;
use crate::style::{parse_number, Style};

const SKIP_TAGS: &[&str] = &[
    "defs",
    "style",
    "title",
    "desc",
    "metadata",
    "clipPath",
    "mask",
    "symbol",
    "linearGradient",
    "radialGradient",
    "filter",
    "marker",
    "pattern",
];

const CONTAINER_TAGS: &[&str] = &["svg", "g", "a", "switch", "use-fallback"];

fn parse_list(value: Option<&str>) -> Vec<f64> {
    match value {
        // SVG number lists separate on whitespace and/or commas.
        Some(v) => v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter_map(parse_number)
            .collect(),
        None => Vec::new(),
    }
}

fn parse_points(value: Option<&str>) -> Vec<(f64, f64)> {
    let nums = parse_list(value);
    nums.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0], c[1]))
        .collect()
}

/// Merge presentation attributes then the `style=` attribute (which wins).
fn apply_style(style: &mut Style, node: &Node) {
    for (k, v) in &node.attrs {
        if k != "style" {
            style.apply(k, v);
        }
    }
    if let Some(s) = node.attr("style") {
        style.apply_style_str(s);
    }
}

fn element_transform(node: &Node, parent: &Transform) -> Transform {
    match node.attr("transform").and_then(|s| Transform::from_str(s).ok()) {
        Some(local) => compose(parent, &local),
        None => parent.clone(),
    }
}

pub fn render(root: &Node, em: &mut Emitter, root_t: &Transform) {
    for child in &root.children {
        render_node(child, &Style::default(), root_t, em);
    }
}

fn render_node(node: &Node, inherited: &Style, parent_t: &Transform, em: &mut Emitter) {
    if SKIP_TAGS.contains(&node.tag.as_str()) {
        return;
    }

    let mut style = inherited.inherit();
    apply_style(&mut style, node);
    let t = element_transform(node, parent_t);
    let m = scale_mag(&t);

    match node.tag.as_str() {
        tag if CONTAINER_TAGS.contains(&tag) => {
            for c in &node.children {
                render_node(c, &style, &t, em);
            }
        }
        "rect" => render_rect(node, &style, &t, m, em),
        "circle" => render_circle(node, &style, &t, m, em),
        "ellipse" => render_ellipse(node, &style, &t, m, em),
        "line" => {
            let pts = [
                (
                    node.attr("x1").and_then(parse_number).unwrap_or(0.0),
                    node.attr("y1").and_then(parse_number).unwrap_or(0.0),
                ),
                (
                    node.attr("x2").and_then(parse_number).unwrap_or(0.0),
                    node.attr("y2").and_then(parse_number).unwrap_or(0.0),
                ),
            ];
            // A straight line encloses no area; SVG fill is meaningless here.
            let mut lstyle = style.clone();
            lstyle.fill = crate::style::Paint::None;
            if let Some(sp) = path::polyline(&pts, &t, false) {
                em.emit_subpaths(&[sp], &lstyle, m);
            }
        }
        "polyline" => {
            let pts = parse_points(node.attr("points"));
            if let Some(sp) = path::polyline(&pts, &t, false) {
                em.emit_subpaths(&[sp], &style, m);
            }
        }
        "polygon" => {
            let pts = parse_points(node.attr("points"));
            if let Some(sp) = path::polyline(&pts, &t, true) {
                em.emit_subpaths(&[sp], &style, m);
            }
        }
        "path" => {
            if let Some(d) = node.attr("d") {
                let subs = path::from_path_d(d, &t);
                em.emit_subpaths(&subs, &style, m);
            }
        }
        "text" => render_text(node, &style, &t, m, em),
        other => {
            debug!("unprocessed element <{}>", other);
            // Unknown containers may still hold drawable children.
            for c in &node.children {
                render_node(c, &style, &t, em);
            }
        }
    }
}

fn render_rect(node: &Node, style: &Style, t: &Transform, m: f64, em: &mut Emitter) {
    let x = node.attr("x").and_then(parse_number).unwrap_or(0.0);
    let y = node.attr("y").and_then(parse_number).unwrap_or(0.0);
    let w = node.attr("width").and_then(parse_number).unwrap_or(0.0);
    let h = node.attr("height").and_then(parse_number).unwrap_or(0.0);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if is_axis_aligned(t) {
        let (x1, y1) = apply(t, x, y);
        let (x2, y2) = apply(t, x + w, y + h);
        em.emit_rect(Pt(x1, y1), Pt(x2, y2), style, m);
    } else {
        em.emit_subpaths(&[path::rect(x, y, w, h, t)], style, m);
    }
}

fn render_circle(node: &Node, style: &Style, t: &Transform, m: f64, em: &mut Emitter) {
    let cx = node.attr("cx").and_then(parse_number).unwrap_or(0.0);
    let cy = node.attr("cy").and_then(parse_number).unwrap_or(0.0);
    let r = node.attr("r").and_then(parse_number).unwrap_or(0.0);
    if r <= 0.0 {
        return;
    }
    if is_similarity(t) {
        let (x, y) = apply(t, cx, cy);
        em.emit_circle(Pt(x, y), r, style, m);
    } else {
        em.emit_subpaths(&[path::ellipse(cx, cy, r, r, t)], style, m);
    }
}

fn render_ellipse(node: &Node, style: &Style, t: &Transform, m: f64, em: &mut Emitter) {
    let cx = node.attr("cx").and_then(parse_number).unwrap_or(0.0);
    let cy = node.attr("cy").and_then(parse_number).unwrap_or(0.0);
    let rx = node.attr("rx").and_then(parse_number).unwrap_or(0.0);
    let ry = node.attr("ry").and_then(parse_number).unwrap_or(rx);
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    em.emit_subpaths(&[path::ellipse(cx, cy, rx, ry, t)], style, m);
}

fn render_text(node: &Node, style: &Style, t: &Transform, m: f64, em: &mut Emitter) {
    let positions = text_positions(node);
    let base = positions.first().copied().unwrap_or((0.0, 0.0));

    if !node.text.is_empty() {
        let (x, y) = apply(t, base.0, base.1);
        em.emit_content(Pt(x, y), style, m, &node.text);
    }

    for child in node.children_named("tspan") {
        let mut tstyle = style.clone();
        apply_style(&mut tstyle, child);
        let cpos = text_positions(child);
        let cpos = if cpos.is_empty() {
            if positions.is_empty() {
                vec![base]
            } else {
                positions.clone()
            }
        } else {
            cpos
        };
        emit_tspan_text(child, &tstyle, t, m, &cpos, em);
    }
}

fn emit_tspan_text(
    child: &Node,
    style: &Style,
    t: &Transform,
    m: f64,
    positions: &[(f64, f64)],
    em: &mut Emitter,
) {
    if child.text.is_empty() {
        return;
    }
    let chars: Vec<char> = child.text.chars().collect();
    if positions.len() > 1 && chars.len() > 1 {
        // Per-glyph positioning: one content call per character.
        for (ch, pos) in chars.iter().zip(positions.iter()) {
            if ch.is_whitespace() {
                continue;
            }
            let (x, y) = apply(t, pos.0, pos.1);
            em.emit_content(Pt(x, y), style, m, &ch.to_string());
        }
    } else {
        let pos = positions.first().copied().unwrap_or((0.0, 0.0));
        let (x, y) = apply(t, pos.0, pos.1);
        em.emit_content(Pt(x, y), style, m, &child.text);
    }
}

fn text_positions(node: &Node) -> Vec<(f64, f64)> {
    let xs = parse_list(node.attr("x"));
    let ys = parse_list(node.attr("y"));
    xs.iter()
        .zip(ys.iter())
        .map(|(x, y)| (*x, *y))
        .collect()
}
