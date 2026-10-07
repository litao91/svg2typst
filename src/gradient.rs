use std::collections::HashMap;

use log::warn;

use crate::dom::Node;
use crate::style::{Rgb, parse_number, parse_paint, Paint};

#[derive(Debug, Clone)]
pub struct Stop {
    pub offset: f64,
    pub color: Rgb,
    pub opacity: f64,
}

#[derive(Debug, Clone)]
pub enum GradientDef {
    Linear {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        user_space: bool,
        stops: Vec<Stop>,
    },
    Radial {
        cx: f64,
        cy: f64,
        r: f64,
        fx: Option<f64>,
        fy: Option<f64>,
        user_space: bool,
        stops: Vec<Stop>,
    },
}

pub type GradientMap = HashMap<String, GradientDef>;

/// Collect every `<linearGradient>`/`<radialGradient>` in the document, keyed
/// by id. Gradients may be referenced before their `<defs>` appear, so this is
/// a separate pre-pass over the whole tree.
pub fn collect(root: &Node) -> GradientMap {
    let mut map = GradientMap::new();
    walk(root, &mut map);
    map
}

fn walk(node: &Node, map: &mut GradientMap) {
    if let Some(id) = node.attr("id") {
        let def = match node.tag.as_str() {
            "linearGradient" => Some(parse_linear(node)),
            "radialGradient" => Some(parse_radial(node)),
            _ => None,
        };
        if let Some(def) = def {
            map.insert(id.to_string(), def);
        }
    }
    for c in &node.children {
        walk(c, map);
    }
}

/// `0.5` or `50%` → 0.5
fn frac(value: Option<&str>, default: f64) -> f64 {
    match value.and_then(parse_number) {
        Some(n) => {
            if value.unwrap_or("").trim_end_matches(|c: char| c.is_whitespace()).ends_with('%') {
                n / 100.0
            } else {
                n
            }
        }
        None => default,
    }
}

fn is_user_space(node: &Node) -> bool {
    node.attr("gradientUnits") == Some("userSpaceOnUse")
}

fn parse_stops(node: &Node) -> Vec<Stop> {
    let mut stops = Vec::new();
    for (i, child) in node.children_named("stop").enumerate() {
        let offset = frac(child.attr("offset"), i as f64);
        let paint = child
            .attr("stop-color")
            .map(|s| parse_paint(s))
            .or_else(|| {
                child.attr("style").and_then(|style| {
                    style
                        .split(';')
                        .find_map(|kv| kv.split_once(':'))
                        .filter(|(k, _)| k.trim() == "stop-color")
                        .map(|(_, v)| parse_paint(v))
                })
            })
            .unwrap_or(Paint::Solid(Rgb { r: 0, g: 0, b: 0 }));
        let color = match paint {
            Paint::Solid(c) => c,
            _ => Rgb { r: 0, g: 0, b: 0 },
        };
        let opacity = child
            .attr("stop-opacity")
            .and_then(parse_number)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        stops.push(Stop {
            offset: offset.clamp(0.0, 1.0),
            color,
            opacity,
        });
    }
    stops
}

fn parse_linear(node: &Node) -> GradientDef {
    GradientDef::Linear {
        x1: frac(node.attr("x1"), 0.0),
        y1: frac(node.attr("y1"), 0.0),
        x2: frac(node.attr("x2"), 1.0),
        y2: frac(node.attr("y2"), 0.0),
        user_space: is_user_space(node),
        stops: parse_stops(node),
    }
}

fn parse_radial(node: &Node) -> GradientDef {
    GradientDef::Radial {
        cx: frac(node.attr("cx"), 0.5),
        cy: frac(node.attr("cy"), 0.5),
        r: frac(node.attr("r"), 0.5),
        fx: node.attr("fx").and_then(parse_number),
        fy: node.attr("fy").and_then(parse_number),
        user_space: is_user_space(node),
        stops: parse_stops(node),
    }
}

impl GradientDef {
    pub fn stops(&self) -> &[Stop] {
        match self {
            GradientDef::Linear { stops, .. } | GradientDef::Radial { stops, .. } => stops,
        }
    }

    pub fn is_user_space(&self) -> bool {
        match self {
            GradientDef::Linear { user_space, .. } | GradientDef::Radial { user_space, .. } => {
                *user_space
            }
        }
    }

    /// Representative solid colour for fallbacks: the first stop.
    pub fn solid_fallback(&self) -> Rgb {
        let stops = self.stops();
        match stops.first() {
            Some(s) => s.color,
            None => Rgb { r: 0, g: 0, b: 0 },
        }
    }
}

fn stop_expr(stop: &Stop, alpha: f64) -> String {
    let a = (stop.opacity * alpha).clamp(0.0, 1.0);
    let color = if a >= 1.0 {
        format!("rgb(\"{}\")", stop.color.hex())
    } else {
        format!(
            "rgb(\"{}\").transparentize({:.2}%)",
            stop.color.hex(),
            (1.0 - a) * 100.0
        )
    };
    format!("({}, {:.3}%)", color, stop.offset * 100.0)
}

/// Emit a native Typst gradient for an objectBoundingBox gradient. Returns
/// `None` when the gradient cannot be represented (userSpaceOnUse, no stops),
/// in which case the caller should fall back to a solid colour.
pub fn to_typst(def: &GradientDef, alpha: f64) -> Option<String> {
    if def.is_user_space() || def.stops().len() < 2 {
        return None;
    }
    let stops: Vec<String> = def.stops().iter().map(|s| stop_expr(s, alpha)).collect();
    let stops = stops.join(", ");
    Some(match def {
        GradientDef::Linear { x1, y1, x2, y2, .. } => {
            // SVG y is down, CeTZ/Typst y is up. `+ 0.0` normalises -0.0.
            let angle = (-(y2 - y1)).atan2(x2 - x1).to_degrees() + 0.0;
            format!("gradient.linear({}, angle: {:.3}deg)", stops, angle)
        }
        GradientDef::Radial { cx, cy, r, fx, fy, .. } => {
            let mut extra = format!(
                "center: ({:.3}%, {:.3}%), radius: {:.3}%",
                cx * 100.0,
                cy * 100.0,
                r * 100.0
            );
            if let (Some(fx), Some(fy)) = (fx, fy) {
                extra.push_str(&format!(
                    ", focal-center: ({:.3}%, {:.3}%)",
                    fx * 100.0,
                    fy * 100.0
                ));
            }
            format!("gradient.radial({}, {})", stops, extra)
        }
    })
}

/// Resolve a paint reference to a Typst fill/stroke expression, warning and
/// falling back to a solid colour for gradients we cannot represent.
pub fn resolve_paint(
    paint: &Paint,
    gradients: &GradientMap,
    alpha: f64,
) -> String {
    match paint {
        Paint::None => "none".to_string(),
        Paint::Solid(c) => solid_expr(*c, alpha),
        Paint::Gradient(id) => match gradients.get(id) {
            Some(def) => match to_typst(def, alpha) {
                Some(expr) => expr,
                None => {
                    warn!(
                        "gradient #{} uses userSpaceOnUse or has too few stops; \
                         falling back to a solid colour",
                        id
                    );
                    solid_expr(def.solid_fallback(), alpha)
                }
            },
            None => {
                warn!("unknown gradient reference #{}", id);
                "none".to_string()
            }
        },
    }
}

pub fn solid_expr(c: Rgb, alpha: f64) -> String {
    let a = alpha.clamp(0.0, 1.0);
    if a >= 1.0 {
        format!("rgb(\"{}\")", c.hex())
    } else {
        format!(
            "rgb(\"{}\").transparentize({:.2}%)",
            c.hex(),
            (1.0 - a) * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dom() -> Node {
        Node::build(
            r##"<svg><defs>
               <linearGradient id="lg" x1="0" y1="0" x2="1" y2="0">
                 <stop offset="0" stop-color="#ff0000"/>
                 <stop offset="1" stop-color="#0000ff" stop-opacity="0.5"/>
               </linearGradient>
               <radialGradient id="rg"><stop offset="0" stop-color="white"/><stop offset="100%" stop-color="black"/></radialGradient>
               <linearGradient id="us" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
             </defs></svg>"##,
        )
        .unwrap()
    }

    #[test]
    fn collects_gradients() {
        let map = collect(&dom());
        assert!(map.contains_key("lg"));
        assert!(map.contains_key("rg"));
        assert!(map.contains_key("us"));
        assert_eq!(map["lg"].stops().len(), 2);
    }

    #[test]
    fn linear_emits_angle_and_stops() {
        let map = collect(&dom());
        let expr = to_typst(&map["lg"], 1.0).unwrap();
        assert!(expr.starts_with("gradient.linear("));
        assert!(expr.contains("angle: 0.000deg"));
        assert!(expr.contains("transparentize(50.00%)"));
    }

    #[test]
    fn radial_emits_center_and_radius() {
        let map = collect(&dom());
        let expr = to_typst(&map["rg"], 1.0).unwrap();
        assert!(expr.contains("center: (50.000%, 50.000%)"));
        assert!(expr.contains("radius: 50.000%"));
    }

    #[test]
    fn user_space_falls_back() {
        let map = collect(&dom());
        assert!(to_typst(&map["us"], 1.0).is_none());
        assert_eq!(map["us"].solid_fallback().hex(), "#ff0000");
    }
}
