use std::collections::HashMap;
use std::str::FromStr;

use log::{debug, warn};
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

const CONTAINER_TAGS: &[&str] = &["svg", "g", "a", "switch"];

/// Guard against `<use>` reference cycles and pathological nesting.
const MAX_USE_DEPTH: usize = 16;

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

fn viewbox_of(node: &Node) -> Option<(f64, f64, f64, f64)> {
    let n: Vec<f64> = node
        .attr("viewBox")?
        .split_whitespace()
        .filter_map(parse_number)
        .collect();
    if n.len() == 4 {
        Some((n[0], n[1], n[2], n[3]))
    } else {
        None
    }
}

fn collect_ids<'a>(node: &'a Node, map: &mut HashMap<String, &'a Node>) {
    if let Some(id) = node.attr("id") {
        map.entry(id.to_string()).or_insert(node);
    }
    for c in &node.children {
        collect_ids(c, map);
    }
}

fn text_positions(node: &Node) -> Vec<(f64, f64)> {
    let xs = parse_list(node.attr("x"));
    let ys = parse_list(node.attr("y"));
    xs.iter().zip(ys.iter()).map(|(x, y)| (*x, *y)).collect()
}

pub struct Renderer<'a> {
    em: &'a mut Emitter,
    ids: HashMap<String, &'a Node>,
}

impl<'a> Renderer<'a> {
    pub fn new(root: &'a Node, em: &'a mut Emitter) -> Self {
        let mut ids = HashMap::new();
        collect_ids(root, &mut ids);
        Renderer { em, ids }
    }

    pub fn run(&mut self, root: &'a Node, root_t: &Transform) {
        for child in &root.children {
            self.render_node(child, &Style::default(), root_t, 1.0, 0);
        }
    }

    fn render_node(
        &mut self,
        node: &'a Node,
        inherited: &Style,
        parent_t: &Transform,
        group_alpha: f64,
        depth: usize,
    ) {
        if SKIP_TAGS.contains(&node.tag.as_str()) {
            return;
        }

        let mut style = inherited.inherit();
        apply_style(&mut style, node);
        let t = element_transform(node, parent_t);
        let m = scale_mag(&t);

        // CeTZ has no group opacity, so fold element/group opacity into the
        // accumulated alpha applied to each descendant's paints.
        let own_opacity = style.opacity;
        style.opacity *= group_alpha;
        let child_alpha = group_alpha * own_opacity;

        match node.tag.as_str() {
            tag if CONTAINER_TAGS.contains(&tag) => {
                for c in &node.children {
                    self.render_node(c, &style, &t, child_alpha, depth);
                }
            }
            "use" => self.render_use(node, &style, &t, child_alpha, depth),
            "rect" => self.render_rect(node, &style, &t, m),
            "circle" => self.render_circle(node, &style, &t, m),
            "ellipse" => self.render_ellipse(node, &style, &t, m),
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
                    self.em.emit_subpaths(&[sp], &lstyle, m);
                }
            }
            "polyline" => {
                let pts = parse_points(node.attr("points"));
                if let Some(sp) = path::polyline(&pts, &t, false) {
                    self.em.emit_subpaths(&[sp], &style, m);
                }
            }
            "polygon" => {
                let pts = parse_points(node.attr("points"));
                if let Some(sp) = path::polyline(&pts, &t, true) {
                    self.em.emit_subpaths(&[sp], &style, m);
                }
            }
            "path" => {
                if let Some(d) = node.attr("d") {
                    let subs = path::from_path_d(d, &t);
                    self.em.emit_subpaths(&subs, &style, m);
                }
            }
            "text" => self.render_text(node, &style, &t, m),
            other => {
                debug!("unprocessed element <{}>", other);
                // Unknown containers may still hold drawable children.
                for c in &node.children {
                    self.render_node(c, &style, &t, child_alpha, depth);
                }
            }
        }
    }

    fn render_use(
        &mut self,
        node: &'a Node,
        style: &Style,
        t: &Transform,
        group_alpha: f64,
        depth: usize,
    ) {
        if depth >= MAX_USE_DEPTH {
            warn!("max <use> depth exceeded; skipping");
            return;
        }
        // Namespace prefixes are stripped by the DOM, so xlink:href == href.
        let href = match node.attr("href") {
            Some(h) => h,
            None => return,
        };
        let id = href.trim_start_matches('#');
        let target = match self.ids.get(id) {
            Some(n) => *n,
            None => {
                warn!("unresolved <use> reference #{}", id);
                return;
            }
        };

        let ux = node.attr("x").and_then(parse_number).unwrap_or(0.0);
        let uy = node.attr("y").and_then(parse_number).unwrap_or(0.0);
        let t = if ux != 0.0 || uy != 0.0 {
            compose(t, &Transform::new(1.0, 0.0, 0.0, 1.0, ux, uy))
        } else {
            t.clone()
        };

        if target.tag == "symbol" {
            self.render_symbol(target, node, style, &t, group_alpha, depth + 1);
        } else {
            self.render_node(target, style, &t, group_alpha, depth + 1);
        }
    }

    /// A referenced `<symbol>` establishes a new viewport: its viewBox is
    /// scaled to the `width`/`height` given on the `<use>` (or the symbol).
    fn render_symbol(
        &mut self,
        symbol: &'a Node,
        use_node: &Node,
        style: &Style,
        t: &Transform,
        group_alpha: f64,
        depth: usize,
    ) {
        let mut st = t.clone();
        if let Some((vx, vy, vw, vh)) = viewbox_of(symbol) {
            let w = use_node
                .attr("width")
                .and_then(parse_number)
                .or_else(|| symbol.attr("width").and_then(parse_number))
                .unwrap_or(vw);
            let h = use_node
                .attr("height")
                .and_then(parse_number)
                .or_else(|| symbol.attr("height").and_then(parse_number))
                .unwrap_or(vh);
            if vw > 0.0 && vh > 0.0 {
                let sx = w / vw;
                let sy = h / vh;
                st = compose(t, &Transform::new(sx, 0.0, 0.0, sy, -vx * sx, -vy * sy));
            }
        }
        for c in &symbol.children {
            self.render_node(c, style, &st, group_alpha, depth);
        }
    }

    fn render_rect(&mut self, node: &Node, style: &Style, t: &Transform, m: f64) {
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
            self.em.emit_rect(Pt(x1, y1), Pt(x2, y2), style, m);
        } else {
            self.em.emit_subpaths(&[path::rect(x, y, w, h, t)], style, m);
        }
    }

    fn render_circle(&mut self, node: &Node, style: &Style, t: &Transform, m: f64) {
        let cx = node.attr("cx").and_then(parse_number).unwrap_or(0.0);
        let cy = node.attr("cy").and_then(parse_number).unwrap_or(0.0);
        let r = node.attr("r").and_then(parse_number).unwrap_or(0.0);
        if r <= 0.0 {
            return;
        }
        if is_similarity(t) {
            let (x, y) = apply(t, cx, cy);
            self.em.emit_circle(Pt(x, y), r, style, m);
        } else {
            self.em.emit_subpaths(&[path::ellipse(cx, cy, r, r, t)], style, m);
        }
    }

    fn render_ellipse(&mut self, node: &Node, style: &Style, t: &Transform, m: f64) {
        let cx = node.attr("cx").and_then(parse_number).unwrap_or(0.0);
        let cy = node.attr("cy").and_then(parse_number).unwrap_or(0.0);
        let rx = node.attr("rx").and_then(parse_number).unwrap_or(0.0);
        let ry = node.attr("ry").and_then(parse_number).unwrap_or(rx);
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        self.em.emit_subpaths(&[path::ellipse(cx, cy, rx, ry, t)], style, m);
    }

    fn render_text(&mut self, node: &Node, style: &Style, t: &Transform, m: f64) {
        let positions = text_positions(node);
        let base = positions.first().copied().unwrap_or((0.0, 0.0));

        if !node.text.is_empty() {
            let pos = if positions.is_empty() {
                vec![base]
            } else {
                positions.clone()
            };
            self.emit_positioned(&node.text, &pos, style, t, m);
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
            self.emit_positioned(&child.text, &cpos, &tstyle, t, m);
        }
    }

    /// Emit text as one block, or one `content` call per glyph when there are
    /// multiple positions and multiple characters.
    fn emit_positioned(
        &mut self,
        text: &str,
        positions: &[(f64, f64)],
        style: &Style,
        t: &Transform,
        m: f64,
    ) {
        if text.is_empty() {
            return;
        }
        let chars: Vec<char> = text.chars().collect();
        if positions.len() > 1 && chars.len() > 1 {
            for (ch, pos) in chars.iter().zip(positions.iter()) {
                if ch.is_whitespace() {
                    continue;
                }
                let (x, y) = apply(t, pos.0, pos.1);
                self.em.emit_content(Pt(x, y), style, m, &ch.to_string());
            }
        } else {
            let pos = positions.first().copied().unwrap_or((0.0, 0.0));
            let (x, y) = apply(t, pos.0, pos.1);
            self.em.emit_content(Pt(x, y), style, m, text);
        }
    }
}

pub fn render(root: &Node, em: &mut Emitter, root_t: &Transform) {
    let mut renderer = Renderer::new(root, em);
    renderer.run(root, root_t);
}
